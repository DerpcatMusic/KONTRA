#!/usr/bin/env python3
"""Read observed UFS2 headers or plain-XML UVIP metadata; never play a patch.

Usage: python3 tools/inspect_uvi.py [--self-test] soundbank.ufs patch.uvip ...
Unknown numeric fields are identified by offset, not assigned guessed meanings.
"""
import argparse
import json
import hashlib
from pathlib import Path
import struct
import xml.etree.ElementTree as ET


HEADER_BYTES = 328
XML_LIMIT = 2 * 1024 * 1024


def parse_header(header):
    if len(header) != HEADER_BYTES:
        raise ValueError("truncated UFS header (328 bytes required)")
    if header[:4] != b"UFS2":
        raise ValueError("unsupported UFS magic (only observed UFS2 layout)")
    field_4 = struct.unpack_from("<I", header, 4)[0]
    field_40 = struct.unpack_from("<Q", header, 40)[0]
    if field_4 != 3 or field_40 != HEADER_BYTES:
        raise ValueError(f"unsupported UFS2 header layout: field_4={field_4}, field_40={field_40}")
    # The name begins at 48 in the observed layout; bytes after its NUL are opaque.
    name, terminator, _ = header[48:304].partition(b"\0")
    if not terminator:
        raise ValueError("unterminated UFS2 name in observed header layout")
    return {
        "magic": "UFS2",
        "field_4_u32le": field_4,
        "field_32_u64le": struct.unpack_from("<Q", header, 32)[0],
        "field_40_u64le": field_40,
        "field_320_u64le": struct.unpack_from("<Q", header, 320)[0],
        "name": name.decode("utf-8", errors="replace"),
        "name_bytes_hex": name.hex(),
        "raw_header_hex": header.hex(),
        "payload_status": "unsupported; directory, programs, scripts and samples not decoded",
        "playback_supported": False,
    }


class BoundedTreeBuilder(ET.TreeBuilder):
    def __init__(self):
        super().__init__()
        self.nodes = 0
        self.depth = 0

    def start(self, tag, attributes):
        # Root depth is zero: at most 65 open elements represent depth <= 64.
        if self.nodes >= 10000 or self.depth >= 65:
            raise ValueError("UVIP XML exceeds depth/node limit during parsing")
        self.nodes += 1
        self.depth += 1
        return super().start(tag, attributes)

    def end(self, tag):
        self.depth -= 1
        return super().end(tag)

    def doctype(self, name, public_id, system_id):
        raise ValueError("UVIP XML declarations with DTD/entities are unsupported")


def parse_program(data):
    if len(data) > XML_LIMIT:
        raise ValueError("UVIP XML exceeds 2 MiB metadata limit")
    try:
        root = ET.fromstring(data, parser=ET.XMLParser(target=BoundedTreeBuilder()))
    except ET.ParseError as error:
        raise ValueError(f"unsupported or malformed plain-XML UVIP: {error}") from error
    if root.tag != "UVI4" or not root.findall("Program"):
        raise ValueError("unsupported UVIP XML root/layout (UVI4/Program required)")
    nodes = []
    pending = [(root, "UVI4")]
    while pending:
        element, location = pending.pop()
        text = (element.text or "").strip().encode("utf-8")
        nodes.append({"path": location, "tag": element.tag, "attributes": element.attrib,
                      "text_bytes": len(text), "text_sha256": hashlib.sha256(text).hexdigest()})
        pending.extend((child, f"{location}/{child.tag}[{index}]")
                       for index, child in reversed(list(enumerate(element))))
    return {"format": "UVI4 plain-XML UVIP", "nodes": nodes,
            "source_sha256": hashlib.sha256(data).hexdigest(),
            "runtime_status": "unsupported; all module attributes/API versions retained as metadata only",
            "playback_supported": False}


def inspect(path):
    with path.open("rb") as source:
        if path.suffix.lower() == ".uvip":
            metadata = parse_program(source.read(XML_LIMIT + 1))
        else:
            metadata = parse_header(source.read(HEADER_BYTES))
    return {"path": str(path), "file_bytes": path.stat().st_size, **metadata}


def self_test():
    # Entirely authored fixture: synthetic numeric/opaque fields, no vendor bytes.
    header = bytearray(HEADER_BYTES)
    header[:4] = b"UFS2"
    struct.pack_into("<I", header, 4, 3)
    struct.pack_into("<QQ", header, 32, 999, HEADER_BYTES)
    header[48:59] = b"Our Fixture"
    header[80:84] = b"\xff\x01\xfe\x02"
    struct.pack_into("<Q", header, 320, 272)
    parsed = parse_header(bytes(header))
    assert parsed["name"] == "Our Fixture" and not parsed["playback_supported"]
    assert parsed["field_32_u64le"] == 999  # Never assumed to be a file size.
    assert bytes.fromhex(parsed["raw_header_hex"]) == bytes(header)
    bad_field = bytearray(header)
    struct.pack_into("<I", bad_field, 4, 4)
    bad_layout = bytearray(header)
    struct.pack_into("<Q", bad_layout, 40, 4096)
    no_name_end = bytearray(header)
    no_name_end[48:304] = b"x" * 256
    program = b'<UVI4><Program Name="Our Patch" FutureKey="opaque"><Layers><Layer Name="ours"><Keygroups><Keygroup><Oscillators><OurUnknownOscillator VendorVersion="99"/></Oscillators></Keygroup></Keygroups></Layer></Layers><EventProcessors><ScriptProcessor API_version="999"><script>-- authored placeholder</script></ScriptProcessor></EventProcessors></Program></UVI4>'
    metadata = parse_program(program)
    assert not metadata["playback_supported"]
    assert metadata["nodes"][1]["attributes"]["FutureKey"] == "opaque"
    assert any(n["tag"] == "OurUnknownOscillator" for n in metadata["nodes"])
    assert any(n["attributes"].get("API_version") == "999" for n in metadata["nodes"])
    assert "authored placeholder" not in json.dumps(metadata)  # No embedded source dump.
    deep = b'<UVI4><Program>' + b'<x>' * 65 + b'</x>' * 65 + b'</Program></UVI4>'
    wide = b'<UVI4><Program>' + b'<x/>' * 10000 + b'</Program></UVI4>'
    utf16_dtd = '<!DOCTYPE UVI4 [<!ENTITY authored "expanded">]><UVI4><Program>&authored;</Program></UVI4>'.encode("utf-16")
    for invalid, expected_nodes in ((deep, 65), (wide, 10000), (utf16_dtd, 0)):
        builder = BoundedTreeBuilder()
        try:
            ET.fromstring(invalid, parser=ET.XMLParser(target=builder))
        except ValueError:
            assert builder.nodes == expected_nodes and builder.depth <= 65
        else:
            raise AssertionError("XML construction was not bounded")
    for invalid in (b'<UVI5><Program/></UVI5>', b'<UVI4/>', b'<UVI4>', deep, wide, utf16_dtd,
                    b'<!DOCTYPE UVI4><UVI4><Program/></UVI4>', b'x' * (XML_LIMIT + 1)):
        try:
            parse_program(invalid)
        except ValueError:
            continue
        raise AssertionError("unsupported or malformed UVIP accepted")
    for invalid in (header[:32], b"UFS1" + header[4:], bad_field, bad_layout, no_name_end):
        try:
            parse_header(bytes(invalid))
        except ValueError:
            continue
        raise AssertionError("unsupported or malformed header accepted")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="*", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
    if not args.paths and not args.self_test:
        parser.error("supply UFS/UVIP files or --self-test")
    failed = False
    for path in args.paths:
        try:
            record = inspect(path)
        except (OSError, ValueError) as error:
            failed = True
            record = {"path": str(path), "error": str(error), "playback_supported": False}
        print(json.dumps(record, ensure_ascii=True))
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
