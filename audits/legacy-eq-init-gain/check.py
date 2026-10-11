"""Source anchors + independent byte model; does NOT execute or compile Rust."""
import hashlib
import json
from pathlib import Path
import struct
import sys

root = Path(__file__).resolve().parents[2]
source = Path(sys.argv[1]) if len(sys.argv) > 1 else root / "crates/sampler-kontakt/src/effects.rs"
text = source.read_text()
for band in range(3):
    assert f'"ENGINE_PAR_GAIN{band + 1}" if group >= 0 && generic == -1 => fx.apply_eq_gain({band}, w.value)' in text
helper = text.split("fn apply_eq_gain(", 1)[1].split("pub(crate) fn params", 1)[0]
for anchor in ["self.params()", "Some(Params::Eq { bands })", "band >= bands.len()", "low: -18.", "high: 18.", ".decode(value)", "8 + band * 12 + 8", "copy_from_slice(&db.to_le_bytes())"]:
    assert anchor in helper, anchor
assert "c.kind != hir::CallbackKind::Init" in (root / "crates/sampler-ksp/src/lib.rs").read_text()
assert "w.group == group && w.generic == generic" in text
assert "slots.iter_mut().find(|fx| fx.slot == i)" in text
assert "(22..=24).contains(&kind)" in text
assert "let at = 8 + band * 12 + 8;" in helper
library = (root / "crates/sampler-kontakt/src/library.rs").read_text()
assert "crate::effects::apply_writes(&mut slots, &self.engine, index as i32, -1)" in library
assert library.index("out.source_modulators(index, group)") < library.index("crate::load::initialize_scripts")


def record(kind):
    return struct.pack("<ii", kind, kind) + b"".join(
        struct.pack("<fff", *v) for v in [(300., 1., 0.), (1000., 1.5, 0.), (6000., 2., 0.)][:kind - 21])


def apply(raw, module, address, write_address, name, value):
    if address != write_address or address[0] < 0 or address[2] != -1 or module != 0x18:
        return raw
    names = ["ENGINE_PAR_GAIN1", "ENGINE_PAR_GAIN2", "ENGINE_PAR_GAIN3"]
    name = name.lstrip("$")
    if name not in names or len(raw) < 8:
        return raw
    kind, repeat = struct.unpack("<ii", raw[:8])
    if kind not in range(22, 25) or repeat != kind or len(raw) != 8 + 12 * (kind - 21):
        return raw
    values = struct.unpack("<" + "f" * ((len(raw) - 8) // 4), raw[8:])
    if any(not (-float("inf") < v < float("inf")) for v in values):
        return raw
    band = names.index(name)
    if band >= kind - 21:
        return raw
    at = 8 + band * 12 + 8
    return raw[:at] + struct.pack("<f", -18. + 36. * max(0, min(value, 1_000_000)) / 1_000_000.) + raw[at + 4:]


cases = 0
address = (7, 5, -1)
for kind in range(22, 25):
    for band in range(3):
        for value in [-2**31, 0, 250_000, 500_000, 750_000, 1_000_000, 2**31 - 1]:
            raw = record(kind)
            result = apply(raw, 0x18, address, address, f"$ENGINE_PAR_GAIN{band + 1}", value)
            at = 16 + 12 * band
            if band < kind - 21:
                assert struct.unpack("<f", result[at:at + 4])[0] == -18. + 36. * max(0, min(value, 1_000_000)) / 1_000_000.
                assert result[:at] == raw[:at] and result[at + 4:] == raw[at + 4:]
            else:
                assert result == raw
            cases += 1
raw = record(22)
for owner in [(8, 5, -1), (7, 4, -1), (7, 5, 1), (-1, 5, 1), (-1, 5, 0), (-1, 5, 1000)]:
    assert apply(raw, 0x18, address, owner, "ENGINE_PAR_GAIN1", 1_000_000) == raw
    if owner[0] < 0 or owner[2] != -1:
        assert apply(raw, 0x18, owner, owner, "ENGINE_PAR_GAIN1", 1_000_000) == raw
    cases += 1
for bad in [raw[:-1], raw + b"\0", raw[:4] + struct.pack("<i", 23) + raw[8:], raw[:8] + struct.pack("<f", float("nan")) + raw[12:]]:
    assert apply(bad, 0x18, address, address, "ENGINE_PAR_GAIN1", 1_000_000) == bad
    cases += 1
for name in ["ENGINE_PAR_GAIN4", "ENGINE_PAR_GAIN01", "ENGINE_PAR_FREQ1", "ENGINE_PAR_BW1"]:
    assert apply(raw, 0x18, address, address, name, 1_000_000) == raw
    cases += 1
assert apply(raw, 0x44, address, address, "ENGINE_PAR_GAIN1", 1_000_000) == raw
result = apply(apply(raw, 0x18, address, address, "ENGINE_PAR_GAIN1", 0), 0x18, address, address, "ENGINE_PAR_GAIN1", 250_000)
assert struct.unpack("<f", result[16:20])[0] == -9.
cases += 2
print(json.dumps({"check": "Python source anchors and independent byte model ONLY", "cases": cases, "effects_sha256": hashlib.sha256(source.read_bytes()).hexdigest(), "Rust": "NOT_RUN/NOT_COMPILED", "native": "UNKNOWN", "CPU_AND_RAM_vs_both_references": "UNACHIEVED"}, indent=2))
