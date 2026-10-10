"""CURRENT13 source pins + independent rational fixtures; no engine execution.

Run: python3 tools/check_event_playpos_current13.py
Every expected number below is synthetic math / candidate geometry, NOT a
Kontakt observation or an assertion of its voice choice, origin or rounding.
"""
from fractions import Fraction as F
from hashlib import sha256
import json
from pathlib import Path

BASE = "13c044d6f9f80576507521a2dc6521e9aad7d9c5"
ROOT = Path(__file__).resolve().parents[1]
PINS = {
    "crates/sampler-core/src/source.rs": "226d325bce409fb0fe0bae840afe86d1db06dba83780904f2a241f02d8ec6a66",
    "crates/sampler-core/src/lib.rs": "ee75e7e280f08a164b262a9ab3f4513c10f564aacc83d676bbe3b5bf75a580c2",
    "crates/sampler-core/src/prepare.rs": "3d8f15ec91044086b7cf63602ff2e376bcb8fc6c5687d63a279e6da423802b60",
    "crates/sampler-core/src/prepare/selection.rs": "5c04db0f0458c03813d43cb23858c39901f62143f58199c9c3207af5b949d3d6",
    "crates/sampler-core/src/note_event.rs": "a6f29cdf88af002a3232bd642fb371635076dc9c1ce8d56ee16e208fd62c28b2",
    "crates/sampler-core/src/script_params.rs": "270a9bef545ecf999d907be627551d7d294370a3c1ee44a27e2105cf48b88d78",
    "crates/sampler-core/src/behavior.rs": "9dc3f5d936d56a0234168c4a75ff992c916497d48880f693593e1c715d9cd678",
    "crates/sampler-ksp/src/lower.rs": "0847275786f1a6f72dc95f09bbfd4dfd65fab3f123a459fd7c3fe53f08d7da1e",
    "crates/sampler-ksp/src/builtins.rs": "305885a31b9ab10db8b9ad03a2853ffd033d163b196063f650ea3a8a165db5cf",
    "crates/sampler-ksp/tests/selection_contract.rs": "7379f02447cdf189703b1478035901d8a1edf4c85279a6285008e3c8f4db47b5",
}

# Independently authored integration input. NOT_RUN / NOT_COMPILED. No expected
# native answer: harness must supply one ordinary zero-origin 44.1-kHz PCM voice,
# 48-kHz output, no loops, envelope long enough to reach the post-wait query.
# Variable selector is assigned in the callback rather than a constant symbol.
KSP_INPUT = """on init
  declare $selector
  declare %positions[3]
end on
on note
  wait(5000)
  $selector := 11
  inc($selector)
  %positions[0] := get_event_par($EVENT_ID,$EVENT_PAR_PLAY_POS)
  %positions[1] := get_event_par($EVENT_ID,12)
  %positions[2] := get_event_par($EVENT_ID,$selector)
end on
"""


def micros(frame, source_rate):
    """Dimensional conversion only; input frame has already been mapped."""
    if source_rate <= 0 or frame < 0:
        raise ValueError("synthetic coordinate requires rate > 0 and frame >= 0")
    return F(frame) * 1_000_000 / source_rate


def run():
    # ponytail: pinned files plus exact strings, not a new Rust parser or build.
    source = {}
    for path, expected in PINS.items():
        data = (ROOT / path).read_bytes()
        assert sha256(data).hexdigest() == expected, (path, "not CURRENT13")
        source[path] = data.decode()
    assert "pub const PLAY_POS: i32 = 12;" in source["crates/sampler-ksp/src/builtins.rs"]
    assert '"$EVENT_PAR_PLAY_POS", event_par::PLAY_POS' in source["crates/sampler-ksp/src/builtins.rs"]
    assert "b::event_par::PLAY_POS" not in source["crates/sampler-ksp/src/lower.rs"]
    assert "7 | 8 | 10 | 11 | 13 | 15 => self.read_event_info(" in source["crates/sampler-core/src/behavior.rs"]
    assert "_ => 0," in source["crates/sampler-core/src/behavior.rs"]

    results = []

    def case(name, coordinate, rate, expected, qualification="synthetic units only"):
        value = micros(coordinate, rate)
        assert value == expected, (name, value, expected)
        results.append({"name": name, "source_frame": str(coordinate),
                        "source_rate_hz": rate, "exact_microseconds": str(value),
                        "floor_candidate": value.numerator // value.denominator,
                        "nearest_half_up_candidate": (value + F(1, 2)).numerator // (value + F(1, 2)).denominator,
                        "qualification": qualification, "native_expected": None})

    offset = F(125 * 44100, 1_000_000)
    assert offset == F(441, 80) and offset % 1 == F(41, 80)
    case("fractional_offset_zero_origin", offset, 44100, F(125))
    assert micros(5, 44100) == F(50000, 441) != F(125)
    step = F(44100, 48000)
    case("source_44100_output_48000", 240 * step, 44100, F(5000))
    case("source_96000_output_48000", 240 * F(96000, 48000), 96000, F(5000))
    case("octave_up_source_clock", 240 * step * 2, 44100, F(10000))
    assert F(240 * 1_000_000, 48000) == 5000  # elapsed output time differs
    case("trim_origin_candidate", 441 + offset, 44100, F(10125),
         "candidate PCM-zero origin; native zone trim origin UNKNOWN")
    reverse = 882 - 1 - offset
    case("reverse_interior_candidate", reverse, 44100, F(700390, 35280) * 1000,
         "candidate end-1 minus fractional distance; native reverse law UNKNOWN")
    # Integer geometry and local continuous candidates are hand-selected, not
    # outputs of a port of Cursor. Avoid asserting a coordinate at discontinuity.
    case("wrap_interior_candidate", F(493, 4), 48000, F(61625, 24),
         "view100..160 loop120..128 distance31.25 maps123.25 under candidate wrap law")
    case("reflected_interior_candidate", F(241, 2), 48000, F(30125, 12),
         "view100..160 loop120..124 distance25.5 maps120.5 under candidate reflection law")
    case("crossfade_primary_candidate", F(253, 2), 48000, F(31625, 12),
         "view100..160 loop120..128 fade4 distance26.5; no selected native leg")
    case("crossfade_partner_candidate", F(237, 2), 48000, F(29625, 12),
         "same synthetic fade; separate partner118.5, never an averaged cursor")
    case("rounding_probe", F(1), 44100, F(10000, 441))
    assert results[-1]["floor_candidate"] == 22
    assert results[-1]["nearest_half_up_candidate"] == 23
    large = F(2**54) + F(1, 4)
    case("large_frame_fraction", large, 44100, F((2**56 + 1) * 2500, 441))
    assert micros(large, 44100) - micros(2**54, 44100) == F(2500, 441)
    # Same note, distinct zones/rates/offsets. Both are valid unit conversions;
    # neither zone ID nor a reduction (min/max/mean/first) supplies native law.
    multi_voice = [
        {"zone": 3, "source_rate_hz": 44100, "frame": str(offset), "microseconds": str(micros(offset, 44100))},
        {"zone": 900, "source_rate_hz": 96000, "frame": "96", "microseconds": str(micros(96, 96000))},
    ]
    assert [v["microseconds"] for v in multi_voice] == ["125", "1000"]
    for frame, rate in [(F(1), 0), (F(-1), 44100)]:
        try:
            micros(frame, rate)
        except ValueError:
            pass
        else:
            raise AssertionError("invalid mathematical input accepted")
    return {"base_commit": BASE, "static_file_pins": "PASS", "synthetic_math": "PASS",
            "case_count": len(results), "cases": results, "multi_voice": multi_voice,
            "ksp_fixture": {"source": KSP_INPUT, "status": "NOT_RUN/NOT_COMPILED",
                            "required_own_assertion_after_implementation": "all three equal and >0 for admitted fixture"},
            "unresolved_native_cases": ["multi-voice reduction", "zero voices/deferred",
                "invalid/retired alias", "stolen/tailing voice eligibility", "cold onset/stream starvation",
                "fractional rounding/width", "zone trim origin", "reverse endpoints",
                "wrap/reflection/count/serial boundaries", "crossfade leg", "wavetable phase"],
            "rust_tests": "NOT_RUN/NOT_COMPILED", "native": "UNKNOWN",
            "cpu_and_ram_vs_both": "UNACHIEVED", "product_code_executed": False}


if __name__ == "__main__":
    print(json.dumps(run(), indent=2))
