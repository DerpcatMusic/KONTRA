#!/usr/bin/env python3
"""Decode Bitwig structure and extract Kontakt states; never claim an inventory is a migration.

python tools/inspect_bitwig.py song.bwproject --copy --extract DIR --importer /path/to/kontakto
Copies remain beside their sources so relative media paths still resolve.
The tagged binary format was cross-checked against jaxter184/bwEdit-Python
and real projects. Unknown tags fail closed; project bytes are never rewritten.
"""

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import zipfile


class Reader:
    """Walk the tagged object stream without reserializing unknown DAW data."""

    SIZES = {1: 1, 2: 2, 3: 4, 4: 8, 5: 1, 6: 4, 7: 8, 11: 4, 21: 16, 22: 16}

    def __init__(self, data, start, end):
        self.data, self.pos, self.end = data, start, end
        self.objects = 0

    def take(self, size):
        if size < 0 or self.pos + size > self.end:
            raise ValueError(f"truncated structure at {self.pos}, requested {size} bytes")
        start = self.pos
        self.pos += size
        return self.data[start:self.pos]

    def u32(self):
        return int.from_bytes(self.take(4), "big")

    def obj(self):
        self.objects += 1
        if self.u32() == 1:
            self.take(4)  # Object reference.
        else:
            self.fields()

    def fields(self):
        while self.u32():
            self.value()

    def value(self):
        tag = self.take(1)[0]
        if tag in self.SIZES:
            self.take(self.SIZES[tag])
        elif tag == 8:
            size = self.u32()
            self.take((size & 0x7fffffff) * (2 if size & 0x80000000 else 1))
        elif tag == 9:
            self.obj()
        elif tag == 10:
            pass
        elif tag == 13:
            size = self.u32()
            nested = self.take(size)
            if nested.startswith(b"BtWg"):
                validate_structure(nested)
        elif tag == 18:
            while self.data[self.pos:self.pos + 4] != b"\0\0\0\3":
                self.obj()
            self.take(4)
        elif tag == 20:
            while self.take(1) != b"\0":
                self.take(self.u32())
                self.obj()
        elif tag in (15, 23):
            self.take(self.u32() * 4)
        elif tag == 25:
            for _ in range(self.u32()):
                self.take(self.u32())
        elif tag == 26:
            count = self.u32()
            if count == 0x90:
                self.fields()  # Inline route object, followed by the route label.
            else:
                self.take(count * 4)
            self.take(self.u32())
        else:
            raise ValueError(f"unsupported Bitwig tag {tag:#x} at {self.pos - 1}")


def validate_structure(data):
    if len(data) < 40 or data[:14] != b"BtWg0003000200":
        raise ValueError("unsupported Bitwig container version")
    start, end = int(data[16:24], 16), int(data[24:40], 16)
    if not 40 <= start <= len(data):
        raise ValueError("invalid Bitwig metadata boundary")
    end = end or len(data)  # Embedded device descriptions have no ZIP boundary.
    if not start <= end <= len(data):
        raise ValueError("invalid Bitwig object boundary")
    reader = Reader(data, start, end)
    while reader.pos < end:
        reader.obj()
    return reader.objects


def vst_component(preset):
    if len(preset) < 48 or preset[:8] != b"VST3\1\0\0\0":
        raise ValueError("unsupported VST3 preset header")
    offset = struct.unpack_from("<Q", preset, 40)[0]
    if offset + 8 > len(preset) or preset[offset:offset + 4] != b"List":
        raise ValueError("invalid VST3 chunk table")
    count = struct.unpack_from("<I", preset, offset + 4)[0]
    if offset + 8 + count * 20 != len(preset):
        raise ValueError("invalid VST3 chunk table size")
    chunks = []
    for index in range(count):
        tag, start, size = struct.unpack_from("<4sQQ", preset, offset + 8 + index * 20)
        if start < 48 or start + size > offset:
            raise ValueError("VST3 chunk extends outside the data section")
        if tag == b"Comp":
            chunks.append(preset[start:start + size])
    if len(chunks) != 1:
        raise ValueError("expected exactly one VST3 component chunk")
    return chunks[0]


def inspect(project, importer=None, extract=None):
    if project.suffix.casefold() != ".bwproject":
        raise ValueError("input must be a .bwproject")
    data = project.read_bytes()
    report = {"source": str(project.resolve()), "source_sha256": hashlib.sha256(data).hexdigest(),
              "status": "inventory_only", "migrated_instances": 0, "kontakt_instances": []}
    try:
        report["decoded_objects"] = validate_structure(data)
        report["structure_decoded"] = True
    except (ValueError, RecursionError) as error:
        report.update(structure_decoded=False, structure_error=str(error))
    with zipfile.ZipFile(project) as archive, tempfile.TemporaryDirectory(prefix="kontra-bitwig-") as work:
        for info in archive.infolist():
            if not info.filename.startswith("plugin-states/") or not info.filename.endswith(".vstpreset"):
                continue
            if info.file_size > 128 * 1024 * 1024:
                raise ValueError("plugin state exceeds 128 MiB limit")
            preset = archive.read(info)  # Also verifies the ZIP CRC.
            try:
                plugin_id = bytes.fromhex(preset[8:40].decode("ascii"))
            except (ValueError, UnicodeError):
                continue
            if b"kontakt" not in plugin_id.lower():
                continue
            component = vst_component(preset)
            state_name = Path(info.filename).name
            instance = {"preset": info.filename, "class_id": preset[8:40].decode("ascii"),
                        "plugin": plugin_id.decode("ascii", "replace"), "state_bytes": len(component),
                        "component_sha256": hashlib.sha256(component).hexdigest()}
            nis = len(component) >= 40 and component[12:16] == b"hsin" and struct.unpack_from("<Q", component)[0] == len(component)
            instance["nis_multi_container"] = nis
            if extract:
                extract.mkdir(parents=True, exist_ok=True)
                for name, content in [(state_name, preset), (state_name + ".nkm", component)] if nis else [(state_name, preset)]:
                    with (extract / name).open("xb") as output:
                        output.write(content)
            if importer and nis:
                path = Path(work, state_name + ".nkm")
                path.write_bytes(component)
                result = subprocess.run([str(importer), "inspect-multi", str(path)], capture_output=True, text=True, timeout=60)
                if result.returncode:
                    instance["import_error"] = result.stderr.strip()
                else:
                    instance["multi"] = json.loads(result.stdout)
                    result = subprocess.run([str(importer), "inspect", str(path)], capture_output=True, text=True, timeout=60)
                    if result.returncode:
                        instance["import_error"] = result.stderr.strip()
                    else:
                        instrument = json.loads(result.stdout)
                        instance.update(instrument=instrument["name"], missing_samples=instrument["missing_samples"],
                                        available_zones=sum(zone["available"] for zone in instrument["zones"]),
                                        zones=len(instrument["zones"]), import_warnings=instrument["warnings"])
            report["kontakt_instances"].append(instance)
    report["limitations"] = ["Kontakt VST3 inventory only; VST2 and CLAP instances are not classified.",
                            "An unchanged source copy is not a KONTRA migration.",
                            "Length-delimited non-BtWg payloads remain opaque.",
                            "Library samples, access data, routing, automation and sonic parity must be verified before replacement."]
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("project", type=Path)
    parser.add_argument("--importer", type=Path, help="KONTRA kontakto CLI for NKM inspection")
    parser.add_argument("--extract", type=Path, help="new directory for exact Kontakt presets and NKM components")
    parser.add_argument("--copy", action="store_true", help="create an unchanged source copy beside the original")
    args = parser.parse_args()
    if args.extract and args.extract.exists():
        parser.error("extraction directory already exists; choose a fresh path")
    copy = args.project.with_name(args.project.stem + " [KONTRA test SOURCE].bwproject")
    if args.copy and (copy.exists() or copy.with_suffix(".inspection.json").exists()):
        parser.error("source copy or report already exists")
    try:
        report = inspect(args.project, args.importer, args.extract)
        if args.copy:
            with args.project.open("rb") as source, copy.open("xb") as output:
                shutil.copyfileobj(source, output)
            digest = hashlib.sha256(copy.read_bytes()).hexdigest()
            if digest != report["source_sha256"]:
                raise ValueError("source changed during copying; discard this test copy")
            report.update(source_copy=str(copy.resolve()), source_copy_verified=True)
            with copy.with_suffix(".inspection.json").open("x") as output:
                json.dump(report, output, indent=2)
        print(json.dumps(report, indent=2))
    except (OSError, ValueError, zipfile.BadZipFile, subprocess.TimeoutExpired) as error:
        parser.exit(2, f"inspect_bitwig: {error}\n")


if __name__ == "__main__":
    main()
