#!/usr/bin/env python3
"""Inspect observed UFS2 records and plain-XML UVIP metadata; never play a patch.

Use --scan --reader UVIWorkstationx64.exe bank.ufs to decode archive names.
Only observed field meanings are reported; index-node contents remain opaque.
"""
import argparse
import json
import hashlib
import os
import re
import shutil
import subprocess
import tempfile
from pathlib import Path
import struct
import xml.etree.ElementTree as ET
import zlib


HEADER_BYTES = 328
XML_LIMIT = 2 * 1024 * 1024
PNG_LIMIT = 64 * 1024 * 1024
RECORD_START = 320
UFS_NAME_BYTES = 256
MURMUR_M = 0xC6A4A7935BD1E995
PCG_MULTIPLIER = 0x5851F42D4C957F2D
U64_MASK = (1 << 64) - 1
READER_NAMESPACE_BYTES = 36
READER_NAMESPACE_PATTERN = re.compile(rb"[ -~]{36}\0")
_READER_NAMESPACE_CACHE = {}


def _mix64(value):
    mixed = value * MURMUR_M & U64_MASK
    return ((mixed ^ (mixed >> 47)) * MURMUR_M) & U64_MASK


def derive_name_key(namespace, bank_name):
    """Derive the observed per-bank filename key without exposing it."""
    words = struct.unpack("<4Q", hashlib.sha256(
        hashlib.sha256(namespace + bank_name).hexdigest().encode("ascii")).digest())
    key = words[0]
    for word in words[1:]:
        key = ((key ^ _mix64(word)) * MURMUR_M) & U64_MASK
    return key


def xor_stream(data, absolute_offset, key):
    """Apply the observed PCG-like XOR stream at a physical bank offset."""
    state = ((_mix64(absolute_offset) ^ key) * MURMUR_M) & U64_MASK
    stream = bytearray()
    for _ in range((len(data) + 3) // 4):
        word = ((state >> 22) ^ state) >> (22 + (state >> 61))
        stream.extend(struct.pack("<I", word & 0xFFFFFFFF))
        state = state * PCG_MULTIPLIER & U64_MASK
    return bytes(left ^ right for left, right in zip(data, stream))


def xor_content_blocks(data, absolute_offset, key):
    """Apply the observed content transform, reseeding every 512 physical bytes."""
    output = bytearray()
    for start in range(0, len(data), 512):
        output.extend(xor_stream(data[start:start + 512], absolute_offset + start, key))
    return bytes(output)


def _png_crc_count(data):
    if not data.startswith(b"\x89PNG\r\n\x1a\n"):
        return 0
    position = 8
    count = 0
    while position + 12 <= len(data):
        length = int.from_bytes(data[position:position + 4], "big")
        end = position + 12 + length
        if end > len(data):
            return 0
        stored_crc = int.from_bytes(data[end - 4:end], "big")
        if zlib.crc32(data[position + 4:end - 4]) & 0xFFFFFFFF != stored_crc:
            return 0
        count += 1
        if data[position + 4:position + 8] == b"IEND":
            return count if end == len(data) and length == 0 else 0
        position = end
    return 0


def _compile_recovery_helper(output):
    source = Path(__file__).with_name("uvi_recover_state.c")
    compiler = shutil.which("cc") or shutil.which("gcc")
    if not compiler or not source.is_file():
        raise ValueError("C compiler or tools/uvi_recover_state.c is unavailable")
    base = [compiler, "-O3", "-std=c11", str(source), "-o", str(output)]
    for flags in (base[:1] + ["-fopenmp"] + base[1:], base):
        result = subprocess.run(flags, capture_output=True, check=False)
        if result.returncode == 0:
            return
    raise ValueError("could not compile the generic stream-state recovery helper")


def _recover_stream_key(helper, encrypted_prefix, known_prefix, absolute_offset, output):
    if len(encrypted_prefix) < 16 or len(known_prefix) < 16:
        raise ValueError("recovery needs 16 encrypted and matching clear bytes")
    outputs = struct.unpack("<4I", bytes(a ^ b for a, b in
                                           zip(encrypted_prefix[:16], known_prefix[:16])))
    words = " ".join(str(value) for value in outputs)
    result = subprocess.run([str(helper), str(absolute_offset), str(output)],
                            input=words, text=True, capture_output=True, check=False)
    if result.returncode != 0:
        raise ValueError("stream-key recovery did not find a matching state")
    key_bytes = Path(output).read_bytes()
    if len(key_bytes) != 8 or Path(output).stat().st_mode & 0o077:
        raise ValueError("recovery helper wrote an invalid or non-private key file")
    key = int.from_bytes(key_bytes, "little")
    if xor_stream(encrypted_prefix[:16], absolute_offset, key) != known_prefix[:16]:
        raise ValueError("recovered key did not reproduce the supplied known plaintext")
    return key


def recover_bank_content_key(bank_path, reader_path, key_output):
    """Find an encrypted PNG, recover the bank stream key, and validate every CRC."""
    bank_path = Path(bank_path)
    scan = scan_ufs(bank_path, reader_path)
    candidates = [entry for entry in scan["records"]
                  if entry.get("kind") == "file" and entry.get("mode") == 2
                  and entry.get("name", "").lower().endswith(".png")
                  and entry.get("data_bounds_valid") and 16 <= entry.get("file_size", 0) <= PNG_LIMIT]
    if not candidates:
        raise ValueError("bank has no bounded encrypted PNG candidate")
    known_prefix = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR"
    key_output = Path(key_output)
    with tempfile.TemporaryDirectory(prefix="uvi-recovery-") as temporary:
        helper = Path(temporary) / "recover"
        _compile_recovery_helper(helper)
        key_path = Path(temporary) / "content-key.bin"
        with bank_path.open("rb") as source:
            for candidate in candidates:
                offset = candidate["data_offset"]
                source.seek(offset)
                encrypted_prefix = source.read(16)
                try:
                    key = _recover_stream_key(helper, encrypted_prefix, known_prefix,
                                              offset, key_path)
                except ValueError:
                    key_path.unlink(missing_ok=True)
                    continue
                source.seek(offset)
                encrypted = source.read(candidate["file_size"])
                if len(encrypted) != candidate["file_size"]:
                    key_path.unlink(missing_ok=True)
                    continue
                clear = xor_content_blocks(encrypted, offset, key)
                crc_count = _png_crc_count(clear)
                if crc_count:
                    descriptor = os.open(key_output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
                    with os.fdopen(descriptor, "wb") as destination:
                        destination.write(key.to_bytes(8, "little"))
                    return {"recovered": True, "key_file": str(key_output),
                            "permissions": "owner-only", "verified_png_chunks": crc_count,
                            "member_offset": offset, "member_bytes": len(encrypted)}
                key_path.unlink(missing_ok=True)
    raise ValueError("candidate PNGs did not pass complete chunk CRC validation")


def self_test_recovery():
    """Exercise the generic C state search with an authored PNG-header fixture."""
    known = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR"
    expected_key = 0x213456789ABCDEF0
    offset = 0x234500
    ciphertext = xor_stream(known, offset, expected_key)
    with tempfile.TemporaryDirectory(prefix="uvi-self-test-") as temporary:
        helper = Path(temporary) / "recover"
        key_path = Path(temporary) / "key.bin"
        _compile_recovery_helper(helper)
        found = _recover_stream_key(helper, ciphertext, known, offset, key_path)
        if found != expected_key:
            raise AssertionError("generic C recovery failed its authored fixture")


def find_reader_namespace(reader_path, bank_name, encrypted_root_name):
    """Find the reader's 36-byte format namespace by matching the bank's Root name.

    This avoids baking the observed namespace into source or output. The namespace
    is read from the supplied official reader binary and never returned to callers.
    """
    path = Path(reader_path).resolve()
    cached = _READER_NAMESPACE_CACHE.get(path)
    if cached is not None:
        key = derive_name_key(cached, bank_name)
        if xor_stream(encrypted_root_name, 332, key).startswith(b"Root\0"):
            return key
        del _READER_NAMESPACE_CACHE[path]
    reader = path.read_bytes()
    if len(reader) < 4096 or reader[:2] != b"MZ":
        raise ValueError("reader must be the official UVI PE executable")
    matches = []
    for candidate in READER_NAMESPACE_PATTERN.finditer(reader):
        namespace = candidate.group()[:READER_NAMESPACE_BYTES]
        key = derive_name_key(namespace, bank_name)
        if xor_stream(encrypted_root_name, 332, key).startswith(b"Root\0"):
            matches.append((namespace, key))
    if len(matches) != 1:
        raise ValueError(f"reader namespace match is not unique ({len(matches)} matches)")
    _READER_NAMESPACE_CACHE[path] = matches[0][0]
    return matches[0][1]


def _record_prefix(source, payload_offset, available):
    source.seek(payload_offset)
    return source.read(min(available, 288))


def scan_ufs(path, reader_path=None, _name_key=None):
    """Walk the observed length-prefixed record stream, preserving unknown fields."""
    path = Path(path)
    file_size = path.stat().st_size
    with path.open("rb") as source:
        header = source.read(HEADER_BYTES)
        metadata = parse_header(header)
        if _name_key is not None:
            name_key = _name_key
        elif reader_path:
            source.seek(RECORD_START + 8 + 4)
            root_cipher = source.read(UFS_NAME_BYTES)
            if len(root_cipher) != UFS_NAME_BYTES:
                raise ValueError("truncated root-name record")
            name_key = find_reader_namespace(reader_path, metadata["name"].encode("utf-8"), root_cipher)
        else:
            name_key = None

        records = []
        cursor = RECORD_START
        expected_chain_end = cursor
        missing_tail = 0
        while cursor < file_size:
            if file_size - cursor < 8:
                missing_tail = file_size - cursor
                expected_chain_end = cursor + missing_tail
                cursor = file_size
                break
            source.seek(cursor)
            declared = struct.unpack("<Q", source.read(8))[0]
            payload_offset = cursor + 8
            available = min(declared, max(0, file_size - payload_offset))
            prefix = _record_prefix(source, payload_offset, available)
            tag = prefix[:4].hex() if len(prefix) >= 4 else ""
            entry = {"record_offset": cursor, "payload_offset": payload_offset,
                     "declared_length": declared, "available_length": available,
                     "tag": tag, "complete": available == declared}
            if tag in ("3236ba2f", "e4505867") and len(prefix) >= 260:
                raw_name = prefix[4:260]
                if name_key is not None:
                    decoded = xor_stream(raw_name, payload_offset + 4, name_key)
                    entry["name"] = decoded.split(b"\0", 1)[0].decode("utf-8", errors="replace")
                else:
                    entry["name_decoded"] = False
                if tag == "3236ba2f" and len(prefix) >= 272:
                    entry["kind"] = "directory"
                    entry["child_descriptor_offset"] = struct.unpack_from("<Q", prefix, 260)[0]
                elif tag == "e4505867" and len(prefix) >= 277:
                    size, data_offset = struct.unpack_from("<QQ", prefix, 260)
                    flag = prefix[276]
                    entry.update({"kind": "file", "file_size": size,
                                  "data_offset": data_offset, "mode": flag,
                                  "content_encrypted": flag == 2})
                    if data_offset > file_size or size > file_size - data_offset:
                        entry["data_bounds_valid"] = False
                    else:
                        entry["data_bounds_valid"] = True
            elif tag == "98b34718" and len(prefix) >= 28:
                pointers = struct.unpack_from("<3Q", prefix, 4)
                entry.update({"kind": "child_descriptor", "index_offset": pointers[0],
                              "first_leaf_offset": pointers[1], "last_leaf_offset": pointers[2],
                              "repeated_pointer": pointers[0] == pointers[1] == pointers[2]})
            elif tag == "af6aa83c" and len(prefix) >= 8:
                entry.update({"kind": "child_table", "declared_child_count":
                              struct.unpack_from("<I", prefix, 4)[0],
                              "table_entries_decoded": name_key is not None})
            elif tag == "ab829c4f" and len(prefix) >= 4:
                entry.update({"kind": "child_index", "index_body_decoded": False})
            else:
                entry["kind"] = "opaque_record"
            records.append(entry)
            if available < declared:
                missing_tail = declared - available
                expected_chain_end = payload_offset + declared
                cursor = payload_offset + available
                break
            cursor = payload_offset + declared
            expected_chain_end = cursor

        tables = {entry["payload_offset"]: entry for entry in records
                  if entry.get("kind") == "child_table"}
        descriptors = {entry["payload_offset"]: entry for entry in records
                       if entry.get("kind") == "child_descriptor"}
        nodes = {entry["payload_offset"]: entry for entry in records
                 if entry.get("kind") in ("directory", "file")}
        for table in tables.values():
            count = table["declared_child_count"]
            required = 24 + count * 264
            if count > 100000 or required > table["declared_length"] or required > table["available_length"]:
                table["table_error"] = "child entries exceed bounded table span"
                continue
            source.seek(table["payload_offset"] + 8)
            raw_entries = source.read(count * 264)
            source.seek(table["payload_offset"] + 8 + count * 264)
            table["previous_leaf_offset"], table["next_leaf_offset"] = struct.unpack("<QQ", source.read(16))
            children = []
            for index in range(count):
                at = index * 264
                entry_offset = table["payload_offset"] + 8 + at
                child_name = None
                if name_key is not None:
                    decoded = xor_stream(raw_entries[at:at + 256], entry_offset, name_key)
                    child_name = decoded.split(b"\0", 1)[0].decode("utf-8", errors="replace")
                metadata_offset = struct.unpack_from("<Q", raw_entries, at + 256)[0]
                target = nodes.get(metadata_offset)
                matches = None if child_name is None else bool(target and target.get("name") == child_name)
                children.append({"name": child_name, "metadata_offset": metadata_offset,
                                 "kind": target.get("kind") if target else None,
                                 "metadata_name_matches": matches})
            table["children"] = children
        for entry in records:
            if entry.get("kind") == "directory":
                descriptor = descriptors.get(entry["child_descriptor_offset"])
                if descriptor:
                    entry["child_index_offset"] = descriptor["index_offset"]
                    entry["first_leaf_offset"] = descriptor["first_leaf_offset"]
                    entry["last_leaf_offset"] = descriptor["last_leaf_offset"]
                    child_rows = []
                    leaf_offsets = []
                    if (descriptor["index_offset"] == U64_MASK and
                            descriptor["first_leaf_offset"] == U64_MASK and
                            descriptor["last_leaf_offset"] == U64_MASK):
                        entry["leaf_chain_valid"] = True
                    else:
                        leaf_offset = descriptor["first_leaf_offset"]
                        previous_offset = U64_MASK
                        visited = set()
                        while leaf_offset in tables and leaf_offset not in visited:
                            table = tables[leaf_offset]
                            if table.get("previous_leaf_offset") != previous_offset:
                                entry["leaf_chain_valid"] = False
                                break
                            visited.add(leaf_offset)
                            leaf_offsets.append(leaf_offset)
                            child_rows.extend(table.get("children", []))
                            if leaf_offset == descriptor["last_leaf_offset"]:
                                entry["leaf_chain_valid"] = table.get("next_leaf_offset") == U64_MASK
                                break
                            previous_offset, leaf_offset = leaf_offset, table.get("next_leaf_offset")
                        else:
                            entry["leaf_chain_valid"] = False
                    entry["leaf_offsets"] = leaf_offsets
                    entry["declared_child_count"] = len(child_rows)
                    entry["children"] = child_rows
                    for child in child_rows:
                        target = nodes.get(child["metadata_offset"])
                        if target:
                            target["parent_offset"] = entry["payload_offset"]

        roots = [node for node in nodes.values()
                 if node.get("kind") == "directory" and node.get("name") == "Root"]
        if len(roots) == 1:
            root = roots[0]
            root["full_path"] = ""
            pending = [root]
            while pending:
                parent = pending.pop()
                for child in parent.get("children", []):
                    target = nodes.get(child["metadata_offset"])
                    if not target or not child.get("metadata_name_matches"):
                        continue
                    target["full_path"] = (parent["full_path"] + "/" + child["name"]).lstrip("/")
                    if target.get("kind") == "directory":
                        pending.append(target)
        return {
            "path": str(path), "file_bytes": file_size,
            "bank_name": metadata["name"], "header": {k: metadata[k] for k in
                ("magic", "field_4_u32le", "field_32_u64le", "field_40_u64le", "field_320_u64le")},
            "record_chain": {"start": RECORD_START, "walked_records": len(records),
                             "observed_end": cursor, "computed_end": expected_chain_end,
                             "declared_end": metadata["field_32_u64le"],
                             "missing_tail_bytes": missing_tail},
            "name_decode": "reader-derived namespace" if name_key is not None else "not requested",
            "records": records,
            "directory_relationships": "decoded from linked 264-byte encrypted-name/clear-pointer leaves; index-node bodies remain opaque",
        }


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
        "payload_status": "header only; use --scan for observed directory and file records",
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

    # Authored record fixture verifies the observed record offsets and bounded walk.
    namespace = bytes(range(36))
    name_key = derive_name_key(namespace, b"Authored")
    sample = b"authored stream test"
    assert xor_stream(xor_stream(sample, 0x12345678, name_key),
                      0x12345678, name_key) == sample
    def png_chunk(tag, body):
        content = tag + body
        return (len(body).to_bytes(4, "big") + content +
                (zlib.crc32(content) & 0xFFFFFFFF).to_bytes(4, "big"))
    png = (b"\x89PNG\r\n\x1a\n" + png_chunk(b"IHDR", bytes(13)) +
           png_chunk(b"IEND", b""))
    encrypted_png = xor_content_blocks(png, 0x12345678, name_key)
    assert xor_content_blocks(encrypted_png, 0x12345678, name_key) == png
    assert _png_crc_count(png) == 2 and _png_crc_count(png[:-1]) == 0
    records = bytearray()
    root_start = RECORD_START
    root_payload = root_start + 8
    descriptor_payload = root_payload + 272 + 8
    table_payload = descriptor_payload + 34 + 8
    file_payload = table_payload + 288 + 8
    member_payload = file_payload + 289 + 8

    def add_record(payload):
        records.extend(struct.pack("<Q", len(payload)))
        records.extend(payload)

    root = bytearray(272)
    root[:4] = bytes.fromhex("3236ba2f")
    root[4:260] = xor_stream(b"Root\0" + bytes(251), root_payload + 4, name_key)
    struct.pack_into("<Q", root, 260, descriptor_payload)
    add_record(root)
    descriptor = bytearray(34)
    descriptor[:4] = bytes.fromhex("98b34718")
    struct.pack_into("<3Q", descriptor, 4, table_payload, table_payload, table_payload)
    add_record(descriptor)
    table = bytearray(288)
    table[:4] = bytes.fromhex("af6aa83c")
    struct.pack_into("<I", table, 4, 1)
    table[8:264] = xor_stream(b"sample.flac\0" + bytes(244), table_payload + 8, name_key)
    struct.pack_into("<Q", table, 264, file_payload)
    struct.pack_into("<2Q", table, 272, U64_MASK, U64_MASK)
    add_record(table)
    file_node = bytearray(289)
    file_node[:4] = bytes.fromhex("e4505867")
    file_node[4:260] = xor_stream(b"sample.flac\0" + bytes(244), file_payload + 4, name_key)
    struct.pack_into("<QQB", file_node, 260, 4, member_payload, 0)
    add_record(file_node)
    add_record(b"DATA")
    archive = bytearray(HEADER_BYTES)
    archive[:4] = b"UFS2"
    struct.pack_into("<I", archive, 4, 3)
    struct.pack_into("<QQ", archive, 32, RECORD_START + len(records), HEADER_BYTES)
    archive[48:57] = b"Authored\0"
    struct.pack_into("<Q", archive, 320, 272)
    import tempfile
    with tempfile.TemporaryDirectory() as temporary:
        fixture = Path(temporary) / "fixture.ufs"
        # The observed header's final u64 overlaps the first record length.
        fixture.write_bytes(archive[:RECORD_START] + records)
        scanned = scan_ufs(fixture, _name_key=name_key)
        root_node = scanned["records"][0]
        file_node = next(row for row in scanned["records"] if row.get("kind") == "file")
        assert root_node["name"] == "Root" and root_node["first_leaf_offset"] == table_payload
        assert root_node["declared_child_count"] == 1
        assert root_node["children"][0]["metadata_name_matches"]
        assert file_node["name"] == "sample.flac" and file_node["file_size"] == 4
        assert file_node["data_offset"] == member_payload and file_node["mode"] == 0
        assert file_node["full_path"] == "sample.flac"
        assert scanned["record_chain"]["observed_end"] == scanned["record_chain"]["declared_end"]
        with fixture.open("rb") as source:
            source.seek(member_payload)
            assert source.read(4) == b"DATA"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="*", type=Path)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--self-test-recovery", action="store_true",
                        help="compile and check the generic C key-recovery helper")
    parser.add_argument("--scan", action="store_true", help="walk observed UFS2 records")
    parser.add_argument("--reader", type=Path, help="official UVI Workstation PE used for name decoding")
    parser.add_argument("--extract-one", type=lambda value: int(value, 0), metavar="DATA_OFFSET",
                        help="extract one clear file member by its absolute data offset")
    parser.add_argument("--output", type=Path, help="new output file for --extract-one")
    parser.add_argument("--recover-content-key", type=Path, metavar="BANK.ufs",
                        help="recover the bank content key from an encrypted PNG and validate all chunk CRCs")
    parser.add_argument("--key-output", type=Path, help="new owner-only file for the recovered 8-byte key")
    args = parser.parse_args()
    if args.self_test:
        self_test()
    if args.self_test_recovery:
        self_test_recovery()
    if not args.paths and not args.self_test and not args.self_test_recovery and args.recover_content_key is None:
        parser.error("supply UFS/UVIP files or --self-test")
    if args.extract_one is not None and (len(args.paths) != 1 or not args.output):
        parser.error("--extract-one requires one bank path and --output")
    if args.recover_content_key is not None:
        if args.paths or not args.reader or not args.key_output:
            parser.error("--recover-content-key requires --reader and --key-output, and no positional paths")
        try:
            result = recover_bank_content_key(args.recover_content_key, args.reader, args.key_output)
            print(json.dumps(result, ensure_ascii=True))
        except (OSError, ValueError) as error:
            print(json.dumps({"error": str(error), "recovered": False}, ensure_ascii=True))
            return 1
        return 0
    failed = False
    for path in args.paths:
        try:
            if args.scan or args.extract_one is not None:
                record = scan_ufs(path, args.reader)
                if args.extract_one is not None:
                    matches = [entry for entry in record["records"]
                               if entry.get("kind") == "file" and
                               entry.get("data_offset") == args.extract_one]
                    if len(matches) != 1:
                        raise ValueError("data offset did not select exactly one file metadata record")
                    member = matches[0]
                    if member["mode"] != 0 or not member["data_bounds_valid"]:
                        raise ValueError("member is not a bounded clear file (mode must be 0)")
                    if args.output.exists():
                        raise ValueError("output already exists; refusing to overwrite")
                    with path.open("rb") as source, args.output.open("xb") as output:
                        source.seek(member["data_offset"])
                        remaining = member["file_size"]
                        while remaining:
                            chunk = source.read(min(1024 * 1024, remaining))
                            if not chunk:
                                raise ValueError("member ended before its declared size")
                            output.write(chunk)
                            remaining -= len(chunk)
                    record["extracted"] = {"name": member.get("name"),
                                           "data_offset": member["data_offset"],
                                           "file_size": member["file_size"],
                                           "output": str(args.output)}
            else:
                record = inspect(path)
        except (OSError, ValueError) as error:
            failed = True
            record = {"path": str(path), "error": str(error), "playback_supported": False}
        print(json.dumps(record, ensure_ascii=True))
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
