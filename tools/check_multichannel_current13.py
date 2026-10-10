#!/usr/bin/env python3
"""CURRENT13 static/fixture checks only; never invokes a Rust compiler or decoder."""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess

BASE = "13c044d6f9f80576507521a2dc6521e9aad7d9c5"
SPECS = {
    "kontakt-formats.html": "46e22eebeb2e1ab7f3799cedbf5011a37209c80896f907e15e981f133dea15de",
    "kontakt-effects.html": "f903ae5b1807ff273324c4f8237174f39f83dc0343536b51d73b477031313644",
}


def chunk(name, data, big=False):
    return name + struct.pack(">I" if big else "<I", len(data)) + data + bytes(len(data) % 2)


def fixture(channels, kind, tail=False):
    values = [
        0 if tail and c < 2 else (c + 1) * 1024 + f * 16
        for f in range(8) for c in range(channels)
    ]
    big = kind in ("aiff", "aifc-float")
    floating = kind in ("wav-float", "aifc-float")
    bits = 32 if floating else 16
    pcm = b"".join(struct.pack(
        (">" if big else "<") + ("f" if floating else "h"),
        value / 32768 if floating else value,
    ) for value in values)
    if kind.startswith("wav"):
        width = bits // 8
        tag = 0xfffe if kind == "wav-extensible" else (3 if floating else 1)
        fmt = struct.pack("<HHIIHH", tag, channels, 48000, 48000 * channels * width,
                          channels * width, bits)
        if tag == 0xfffe:
            # Six/eight-channel masks are fixtures, not claims about Kontakt routing.
            mask = (1 << channels) - 1
            fmt += struct.pack("<HHI", 22, bits, mask)
            fmt += bytes.fromhex("0100000000001000800000aa00389b71")
        body = b"WAVE" + chunk(b"fmt ", fmt) + chunk(b"data", pcm)
        return chunk(b"RIFF", body), values
    comm = struct.pack(">HIH", channels, 8, bits) + bytes.fromhex("400ebb80000000000000")
    if kind != "aiff":
        codec = b"fl32" if floating else b"sowt"
        comm += codec + b"\x00\x00"  # Empty padded compression name.
    body = (b"AIFF" if kind == "aiff" else b"AIFC")
    if kind != "aiff":
        body += chunk(b"FVER", struct.pack(">I", 0xa2805140), True)
    body += chunk(b"COMM", comm, True) + chunk(b"SSND", bytes(8) + pcm, True)
    return chunk(b"FORM", body, True), values


def inspect_fixture(data, kind):
    big = data.startswith(b"FORM")
    order = ">" if big else "<"
    assert struct.unpack_from(order + "I", data, 4)[0] == len(data) - 8
    at, chunks = 12, {}
    while at < len(data):
        size = struct.unpack_from(order + "I", data, at + 4)[0]
        body = data[at + 8:at + 8 + size]
        assert len(body) == size
        chunks[data[at:at + 4]] = body
        at += 8 + size + size % 2
    assert at == len(data)
    if big:
        channels, count, bits = struct.unpack_from(">HIH", chunks[b"COMM"])
        assert count == 8
        assert chunks[b"COMM"][8:18] == bytes.fromhex("400ebb80000000000000")
        assert chunks[b"SSND"][:8] == bytes(8)
        pcm = chunks[b"SSND"][8:]
    else:
        tag, channels, rate, byte_rate, align, bits = struct.unpack_from("<HHIIHH", chunks[b"fmt "])
        assert rate == 48000 and align == channels * bits // 8
        assert byte_rate == rate * align
        if kind == "wav-extensible":
            assert tag == 0xfffe and len(chunks[b"fmt "]) == 40
        pcm = chunks[b"data"]
    floating = kind in ("wav-float", "aifc-float")
    pcm_order = "<" if kind == "aifc-sowt" else order
    values = [v[0] for v in struct.iter_unpack(pcm_order + ("f" if floating else "h"), pcm)]
    assert len(values) == 8 * channels
    return channels, [v if floating else v / 32768 for v in values]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    for name, digest in SPECS.items():
        assert hashlib.sha256((args.archive / name).read_bytes()).hexdigest() == digest
    paths = {
        "samples": "crates/sampler-kontakt/src/samples.rs",
        "stream": "crates/sampler-kontakt/src/stream.rs",
        "pcm": "crates/sampler-kontakt/src/pcm.rs",
        "effects": "crates/sampler-kontakt/src/effects.rs",
        "headers": "crates/sampler-kontakt/src/header_cache.rs",
        "core": "crates/sampler-core/src/lib.rs",
        "ir": "crates/sampler-ir/src/lib.rs",
    }
    sources, hashes = {}, {}
    for label, path in paths.items():
        data = (root / path).read_bytes()
        baseline = subprocess.check_output(["git", "-C", str(root), "show", f"{BASE}:{path}"])
        assert data == baseline, f"reviewed product source changed: {path}"
        sources[label] = data.decode()
        hashes[path] = hashlib.sha256(data).hexdigest()
    assert "self.channels.min(2) - 1" in sources["samples"]
    assert "channels.min(2) - 1" in sources["samples"]
    assert "channels == 0 ||" in sources["samples"]
    assert "reader.header.channels.min(2)" in sources["stream"]
    assert sources["stream"].index("let pcm = head.starts_with") < sources["stream"].index("if pcm {")
    assert "return Ok(Self {" in sources["stream"].split("if pcm {", 1)[1].split("bytes.seek(SeekFrom::Start(0))?;\n        if head.starts_with", 1)[0]
    assert "Only mono/stereo samples are supported" in sources["pcm"]
    assert "pub type Frame = [f32; 2]" in sources["core"]
    assert "pub left: Vec<f32>" in sources["ir"] and "pub right: Vec<f32>" in sources["ir"]
    assert "Vec<[f32; 2]>" in sources["effects"]
    assert 'KONTRA v2 numeric headers 1' in sources["headers"]
    cases = []
    for kind in ("wav", "wav-extensible", "wav-float", "aiff", "aifc-sowt", "aifc-float"):
        for channels in (1, 2, 3, 4, 6, 8):
            for tail in (False, True):
                data, markers = fixture(channels, kind, tail)
                count, values = inspect_fixture(data, kind)
                assert count == channels and values == [v / 32768 for v in markers]
                frames = [values[i:i + channels] for i in range(0, len(values), channels)]
                selected = [[f[0], f[min(2, channels) - 1]] for f in frames]
                if tail and channels > 2:
                    assert all(s == [0, 0] for s in selected)
                    assert any(v != 0 for f in frames for v in f[2:])
                cases.append({"kind": kind, "channels": channels, "tail_only": tail,
                              "sha256": hashlib.sha256(data).hexdigest()})
    print(json.dumps({"source": BASE, "status": "PASS_STATIC_AND_SYNTHETIC_ONLY",
                      "product_decoders_executed": False, "rust": "NOT_RUN/NOT_COMPILED",
                      "native": "UNKNOWN", "cpu_and_ram_vs_both_references": "UNACHIEVED",
                      "source_sha256": hashes, "spec_sha256": SPECS,
                      "fixture_cases": len(cases), "fixture_case_digest": hashlib.sha256(
                          json.dumps(cases, sort_keys=True).encode()).hexdigest()}, indent=2))


if __name__ == "__main__":
    main()
