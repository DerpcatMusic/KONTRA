#!/usr/bin/env python3
"""Write docs/re/kontakt-bindings/audit-probes.tsv: one row per mapping-audit probe request.

    audit_probes.py kontakt-oracle-probes.tsv OUT.tsv

Rows we measured carry PROVED / DEBUNKED / PARTIAL and the measured law; the rest are triaged by what
their proof_method needs (the oracle is state-only: Lua API + KSP `on init` + save + decode; it cannot
render audio or deliver MIDI), so they are reported as blocked rather than guessed.
"""
import re
import sys

B = "docs/re/kontakt-bindings/"
P = "tools/kontakt-oracle/probes/audit_levels.lua"
FX = "tools/kontakt-oracle/probe_fx.py"
MEASURED = {
    "KE154": ("PROVED", "stored = 16*n^3 for INSERT_EFFECT_OUTPUT_GAIN and SEND_EFFECT_DRY_LEVEL (n=value/1e6); unity (stored 1.0) at value 396851; clamped to [0,1e6]; stored is a linear gain", FX + " -> " + B + "filters_eq.tsv, fx_*.tsv"),
    "KE012": ("PROVED", "wrapper output_gain/dry_level are linear gains: stored = 16*n^3, so 1.0 = value 396851 (see KE154)", FX + " -> " + B + "fx_other.tsv"),
    "KE068": ("PROVED", "legacy EQ band gain stored dB = -18 + 36*n, clamped to n in [0,1] (value<0 -> -18, >1e6 -> +18)", FX + " -> " + B + "filters_eq.tsv (Filters/EQ GAIN1)"),
    "KE107": ("PROVED", "EQ gain clamp [-18,+18] dB holds for out-of-range engine values; Solid G-EQ gains are stored normalized 0..1 (identity), not dB", B + "engine_par_clamp.tsv"),
    "KL040": ("PROVED", "EQ frequency stored Hz = 20*1000^n = 20*10^(3n) (n=log10(hz/20)/3); NOT clamped outside [0,1] (-500000 -> 0.632 Hz, 2e6 -> 2e7 Hz)", B + "engine_par_clamp.tsv, filters_eq.tsv (FREQ1)"),
    "KL055": ("DEBUNKED", "ENGINE_PAR values are clamped to [0,1e6] per parameter, not universally: EQ gain, EQ BW, compressor, output gain clamp; EQ FREQ1 and ACBox AC_BASS extrapolate (AC_BASS -500000 -> -0.5, 2e6 -> 2.0)", B + "engine_par_clamp.tsv"),
    "KE088": ("PROVED", "Compressor v0x70 field 2 (RATIO) stored = 0.04^n, i.e. inverse ratio from 1 (1:1) to 0.04 (25:1); THRESHOLD field1 = -68 + 65*n dB; clamped to n in [0,1]", B + "fx_dynamics_dist_tape.tsv, engine_par_clamp.tsv"),
    "KE150": ("PROVED", "Compressor v0x70 fields: 1 THRESHOLD dB(-68+65n), 2 RATIO(0.04^n), 3 ATTACK ms (1..1000, curve), 4 DECAY ms (50..2500, curve); FBComp v0x10: 0 input,1 makeup,2 attack,3 release,4 ratio,5 mix (identity), 7 HQ bool; SSL bus comp v0x12: 0 threshold,1 makeup(identity),2 attack,3 release,4 ratio (6-step enums 0..5/6),5 mix", B + "fx_dynamics_dist_tape.tsv, curves_dense.tsv"),
    "KE151": ("PROVED", "GaloisReverb v0x10 fields (all normalized identity): 1 TIME,2 SIZE,3 DAMPING,4 MOD,5 DIFF,6 PREDELAY,7 HIGHCUT,8 LOWSHELF,9 STEREO", B + "fx_legacy_delay_reverb_mod.tsv (generic=1)"),
    "KE091": ("PARTIAL", "stored Galois reverb controls are normalized identity (stored = n); the physical decay/size laws need audio and are not measured", B + "fx_legacy_delay_reverb_mod.tsv"),
    "KE118": ("PARTIAL", "same as KE091: stored = n; display/physical laws unmeasured", B + "fx_legacy_delay_reverb_mod.tsv"),
    "KE079": ("PARTIAL", "EFFECT_SUBTYPE accepted filter type ids: 0,2,3,4,6-17,19,22,23,24,30-41,50-65,70,71,90,91,100-108,120,130,140; all others are coerced to 0 by Kontakt's own setter", B + "filter_subtypes.tsv"),
    "KE078": ("PARTIAL", "ids 52,54,55,57 are valid Kontakt filter types (accepted by the setter); DSP laws unmeasured (needs audio)", B + "filter_subtypes.tsv"),
    "KE083": ("PARTIAL", "ladder ids 30..41 are all valid filter types; DSP unmeasured", B + "filter_subtypes.tsv"),
    "KE085": ("PARTIAL", "Daft ids 70,71 are valid; DSP unmeasured", B + "filter_subtypes.tsv"),
    "KE081": ("PARTIAL", "filter type 13 is valid (cutoff identity, resonance identity bindings); DSP unmeasured", B + "filter_subtypes.tsv, filters_eq.tsv"),
    "KE082": ("PARTIAL", "ids 90 and 91 (Formant I/II) are valid; DSP unmeasured", B + "filter_subtypes.tsv"),
    "KL082": ("PROVED", "zone tune stored/getter in semitones, range [-36,36], KONTRA IR agrees on 14 values incl. 0.01, 0.5, 7.5, 12, 36 (group tune likewise); instrument tune + midi_transpose are folded into KONTRA zone tune", P + " + tools/kontakt-oracle/audit_levels.sh (2001 compared, 0 mismatches)"),
    "KL085": ("PARTIAL", "stored zone/group/instrument pan equals the API percent (-100..100) for 9 values, KONTRA agrees; API range is [-100,100] for each; the summed-pan clamp needs audio", P),
    "KL095": ("PROVED", "gain = 2^(dB/6) at group (range -inf..12), zone (API range [-36,36]) and instrument (-inf..12) level; group volume -200 dB reads back -inf (stored 0 below a floor)", P + " (0 mismatches except the -inf floor)"),
    "KL013": ("PROVED", "instrument/program volume is dB with 6 dB per doubling (same law), API range -inf..12", P),
    "KL033": ("PARTIAL", "Kontakt voice-group exclusive_group domain via API: 0..16 (0 = none, reads back nil; 1..16 valid); 127 rejected", P),
    "KL034": ("PROVED", "voice fade_time domain 0..1000 ms; negative values are rejected by Kontakt (not stored); voices domain 1..128", P),
    "KL011": ("PROVED", "default key switch domain 0..127; Kontakt rejects -1 and 128", P),
    "KL080": ("PROVED", "loop modes off/until_end/until_end_alt/until_release/until_release_alt round-trip; KONTRA loop count/start/end/alternating/until_release match for all five", P),
    "KL081": ("PROVED", "until_release and until_release_alt map to release-terminated loops (diff zone.loop_until_release: 0 mismatches)", P),
    "KL022": ("PARTIAL", "playback modes sampler/dfd/time_machine_1/2/pro/beat_machine/mpc60_machine/s1200_machine all round-trip and KONTRA reads them; DFD-vs-sampler audio unmeasured", P),
}
KEYS = [
    ("BLOCKED-AUDIO", r"impulse|PCM|audio|RMS|sweep|render|tone wav|output landmark|decay|latency|underrun|ramp|saturation|transfer|vector"),
    ("BLOCKED-RUNTIME", r"KSP|script|callback|note-off|pedal|MIDI|keyswitch|persistent|ui_|UI|round robin|release|trigger|voice allocator|steal"),
    ("STATIC-QUERY", r"^(Static|Query|Trace|Native .*query|Source|Existing)"),
]


def main() -> int:
    rows = [l.split("\t") for l in open(sys.argv[1]).read().splitlines() if l.strip()]
    out = ["id\tstatus\tmeasured_law\tprobe_file\tnative_thing"]
    for r in rows:
        rid, thing, method = r[1], r[3], r[8]
        if rid in MEASURED:
            st, law, probe = MEASURED[rid]
        else:
            st = next((s for s, rx in KEYS if re.search(rx, method, re.I)), "NOT-PROBED")
            law = {"BLOCKED-AUDIO": "needs an audio render of Kontakt; the oracle is state-only (Lua API + save + decode)",
                   "BLOCKED-RUNTIME": "needs MIDI/script runtime events; the oracle only runs `on init`",
                   "STATIC-QUERY": "binary query rather than oracle probe",
                   "NOT-PROBED": "no state-only probe designed yet"}[st]
            probe = ""
        out.append("\t".join([rid, st, law, probe, thing]))
    open(sys.argv[2], "w").write("\n".join(out) + "\n")
    from collections import Counter
    print(Counter(l.split("\t")[1] for l in out[1:]))
    return 0


if __name__ == "__main__":
    sys.exit(main())
