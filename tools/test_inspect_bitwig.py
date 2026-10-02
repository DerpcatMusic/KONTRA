"""Run: python tools/test_inspect_bitwig.py"""

from pathlib import Path
import struct
import tempfile
import zipfile
from unittest.mock import patch

from inspect_bitwig import inspect, validate_structure, vst_component
from migrate_bitwig import KONTRA_ID, migrate


def project_bytes(tree):
    return b"BtWg0003000200ba00000028" + f"{40 + len(tree):016x}".encode() + tree


def check():
    # Exercise UTF-16 lengths, nested containers, an inline route, and opaque data.
    nested = project_bytes(struct.pack(">II", 46, 0))
    field = lambda number, value: struct.pack(">I", number) + value
    tree = struct.pack(">I", 46)
    tree += field(1, b"\x08" + struct.pack(">I", 0x80000002) + "אב".encode("utf-16be"))
    tree += field(2, b"\x0d" + struct.pack(">I", len(nested)) + nested)
    tree += field(3, b"\x1a" + struct.pack(">II", 0x90, 0) + struct.pack(">I", 3) + b"361")
    tree += field(4, b"\x0d" + struct.pack(">I", 3) + b"raw")
    tree += struct.pack(">I", 0)
    data = project_bytes(tree)
    assert validate_structure(data) == 1
    for malformed in (data[:-1], project_bytes(struct.pack(">II", 46, 1) + b"\xff")):
        try:
            validate_structure(malformed)
        except ValueError:
            pass
        else:
            raise AssertionError("malformed structure accepted")
    component = struct.pack("<QI", 40, 1) + b"hsin" + bytes(24)
    cid = b"5653544E694B376B6F6E74616B742037"
    preset = b"VST3\1\0\0\0" + cid + struct.pack("<Q", 48 + len(component)) + component
    preset += b"List" + struct.pack("<I4sQQ", 1, b"Comp", 48, len(component))
    assert vst_component(preset) == component
    with tempfile.TemporaryDirectory() as work:
        path = Path(work, "song.bwproject")
        path.write_bytes(data)
        with zipfile.ZipFile(path, "a") as archive:
            archive.writestr("plugin-states/example.vstpreset", preset)
        before = path.read_bytes()
        report = inspect(path, extract=Path(work, "states"))
        assert path.read_bytes() == before
        assert report["structure_decoded"] and report["migrated_instances"] == 0
        assert len(report["kontakt_instances"]) == 1
        assert Path(work, "states/example.vstpreset.nkm").read_bytes() == component
        # Replace one device while retaining an unrelated state and all source bytes.
        string = lambda value: b"\x08" + struct.pack(">I", len(value)) + value
        nested = project_bytes(struct.pack(">I", 1453) + field(1, string(b"/old/Kontakt.vst3\n" + cid)) + bytes(4))
        device = (struct.pack(">I", 1832) + field(1, string(b"example.vstpreset"))
                  + field(2, string(cid)) + field(3, string(b"Kontakt 7"))
                  + field(4, b"\x0d" + struct.pack(">I", len(nested)) + nested) + bytes(4))
        path.write_bytes(project_bytes(device))
        with zipfile.ZipFile(path, "a") as archive:
            archive.writestr("plugin-states/example.vstpreset", preset)
            archive.writestr("plugin-states/unrelated", b"keep exactly")
        before = path.read_bytes()
        plugin = Path(work, "KONTRA.vst3")
        plugin.mkdir()
        destination = Path(work, "copy.bwproject")
        state = b"verified-state"
        with patch("migrate_bitwig.export_state", return_value=(state, {"imported_parts": []})):
            migrate(path, destination, {"example.vstpreset": Path(work, "multi")}, Path(work, "exporter"), plugin)
        assert path.read_bytes() == before
        assert validate_structure(destination.read_bytes()) == 1
        assert KONTRA_ID in destination.read_bytes() and cid not in destination.read_bytes()
        with zipfile.ZipFile(destination) as archive:
            assert vst_component(archive.read("plugin-states/example.vstpreset")) == state
            assert archive.read("plugin-states/unrelated") == b"keep exactly"
        try:
            migrate(path, destination, {}, Path(work, "exporter"), plugin)
        except ValueError:
            pass
        else:
            raise AssertionError("existing output overwritten")
    print("Bitwig structure, malformed input, VST3 extraction and source preservation: OK")


if __name__ == "__main__":
    check()
