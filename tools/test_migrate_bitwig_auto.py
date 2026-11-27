"""Run: python3 plugins/kontra/tools/test_migrate_bitwig_auto.py"""

from __future__ import annotations

import json
import struct
import tempfile
import zipfile
from pathlib import Path
from unittest.mock import patch

from inspect_bitwig import validate_structure, vst_component
from migrate_bitwig_auto import (
    DEFAULT_KONTRA_CID,
    build_device_replacements,
    resolve_kontra_cid,
    rewrite_project,
    vst3_preset,
)


def project_bytes(tree: bytes) -> bytes:
    return b"BtWg0003000200ba00000028" + f"{40 + len(tree):016x}".encode() + tree


def field(number: int, value: bytes) -> bytes:
    return struct.pack(">I", number) + value


def bstr(value: bytes) -> bytes:
    return b"\x08" + struct.pack(">I", len(value)) + value


def check() -> None:
    kontakt_cid = b"5653544E694B386B6F6E74616B742038"
    comp = struct.pack("<QI", 40, 1) + b"hsin" + bytes(24)
    kontakt_preset = vst3_preset(comp, kontakt_cid)
    assert vst_component(kontakt_preset) == comp

    with tempfile.TemporaryDirectory() as work_str:
        work = Path(work_str)
        plugin = work / "KONTRA.vst3"
        modinfo = plugin / "Contents" / "Resources" / "moduleinfo.json"
        modinfo.parent.mkdir(parents=True)
        modinfo.write_text(
            json.dumps({"Classes": [{"CID": DEFAULT_KONTRA_CID.decode("ascii")}]}),
            encoding="utf-8",
        )
        assert resolve_kontra_cid(plugin) == DEFAULT_KONTRA_CID

        nested = project_bytes(
            struct.pack(">I", 1453)
            + field(
                1,
                bstr(
                    b"C:\\Program Files\\Common Files\\VST3\\Kontakt 8 Portable\\Kontakt 8 Portable.vst3\n"
                    + kontakt_cid
                ),
            )
            + field(2, bstr(b"Kontakt 8 Portable/Kontakt 8 Portable.vst3"))
            + field(3, bstr(b"Native Instruments"))
            + field(4, bstr(b"C:\\Program Files\\Common Files\\VST3"))
            + bytes(4)
        )
        device = (
            struct.pack(">I", 1832)
            + field(1, bstr(b"k8.vstpreset"))
            + field(2, bstr(kontakt_cid))
            + field(3, bstr(b"Kontakt 8"))
            + field(4, b"\x0d" + struct.pack(">I", len(nested)) + nested)
            + bytes(4)
        )
        src = work / "song.bwproject"
        src.write_bytes(project_bytes(device))
        with zipfile.ZipFile(src, "a") as archive:
            archive.writestr("plugin-states/k8.vstpreset", kontakt_preset)
            archive.writestr("plugin-states/keep.bin", b"untouched")

        repls = build_device_replacements(device, kontakt_cid, plugin, DEFAULT_KONTRA_CID)
        assert repls[kontakt_cid] == DEFAULT_KONTRA_CID
        assert repls[b"Kontakt 8"] == b"KONTRA"
        assert repls[b"Native Instruments"] == b"Matari Audio"

        fake_oast = b"OAST\x01\x00\x00\x00test-state-payload"
        dst = work / "song [KONTRA].bwproject"
        before = src.read_bytes()
        out = rewrite_project(
            src,
            dst,
            {"plugin-states/k8.vstpreset": (kontakt_cid, fake_oast)},
            plugin,
            DEFAULT_KONTRA_CID,
            in_place=False,
        )
        assert out["rewritten_entries"] == 1
        assert src.read_bytes() == before
        rewritten_bytes = dst.read_bytes()
        assert validate_structure(rewritten_bytes) == 1
        assert DEFAULT_KONTRA_CID in rewritten_bytes
        assert kontakt_cid not in rewritten_bytes
        assert b"Matari Audio" in rewritten_bytes
        assert b"Native Instruments" not in rewritten_bytes
        with zipfile.ZipFile(dst) as archive:
            assert vst_component(archive.read("plugin-states/k8.vstpreset")) == fake_oast
            assert archive.read("plugin-states/keep.bin") == b"untouched"

        # Test --in-place creates a one-time .pre-kontra-migrate backup.
        out_ip = rewrite_project(
            src,
            src,
            {"plugin-states/k8.vstpreset": (kontakt_cid, fake_oast)},
            plugin,
            DEFAULT_KONTRA_CID,
            in_place=True,
        )
        assert out_ip["rewritten_entries"] == 1
        backup = work / "song.bwproject.pre-kontra-migrate"
        assert backup.is_file() and backup.read_bytes() == before
        assert validate_structure(src.read_bytes()) == 1

    print("test_migrate_bitwig_auto: OK")


if __name__ == "__main__":
    check()
