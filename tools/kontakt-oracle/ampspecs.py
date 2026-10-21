#!/usr/bin/env python3
"""Write stimrender.py specs for the Kontakt amp and reverb modules.

    ampspecs.py jump|van51|reverbdelight OUT.json

Each spec sweeps every knob one at a time (low, mid, high; booleans off/on) over the
`ramp` stimulus (200 Hz, 0 -> 1.0), with the other knobs at mid. Values are on the
engine's 0..1000000 scale.
"""
import json
import sys

M = 1000000

# name -> (kind, slot): kind "f" float knob, "b" boolean. Names as in the
# $ENGINE_PAR_* bindings (docs/re/kontakt-bindings/fx_*.tsv).
MODULES = {
    "jump": {
        "chain": ["Effects/Jump"],
        "knobs": [("JMP_PREAMP", "f"), ("JMP_BASS", "f"), ("JMP_MID", "f"), ("JMP_TREBLE", "f"),
                  ("JMP_PRESENCE", "f"), ("JMP_MASTER", "f"), ("JMP_MONO", "b")],
    },
    "van51": {
        "chain": ["Effects/Van51"],
        "knobs": [("V5_PREGAINRHYTHM", "f"), ("V5_PREGAINLEAD", "f"), ("V5_BASS", "f"), ("V5_MID", "f"),
                  ("V5_TREBLE", "f"), ("V5_POSTGAIN", "f"), ("V5_RESONANCE", "f"), ("V5_PRESENCE", "f"),
                  ("V5_LEADCHANNEL", "b"), ("V5_HIGAIN", "b")],
    },
    "reverbdelight": {
        "chain": ["Effects/ReverbDelight"],
        "knobs": [("REVERBDELIGHT_VOLUME", "f"), ("REVERBDELIGHT_BASS", "f"), ("REVERBDELIGHT_TREBLE", "f"),
                  ("REVERBDELIGHT_BRIGHT", "b"), ("REVERBDELIGHT_TREM_ON", "b"), ("REVERBDELIGHT_TREM_SPEED", "f"),
                  ("REVERBDELIGHT_TREM_DEPTH", "f"), ("REVERBDELIGHT_TREM_SAG", "f"), ("REVERBDELIGHT_VERB_ON", "b"),
                  ("REVERBDELIGHT_VERB_TYPE", "b"), ("REVERBDELIGHT_VERB_VOLUME", "f")],
    },
}


def spec(name):
    m = MODULES[name]
    pre = [[0, k, M // 2] for k, kind in m["knobs"] if kind == "f"]
    pre += [[0, k, 0] for k, kind in m["knobs"] if kind == "b"]
    steps = [{"name": "default", "set": []}]
    for k, kind in m["knobs"]:
        levels = [0, M // 2, M] if kind == "f" else [0, M]
        for v in levels:
            steps.append({"name": f"{k}={v}", "set": [[0, k, v]]})
    return {"chain": m["chain"], "pre": pre, "steps": steps, "stimulus": "ramp"}


if __name__ == "__main__":
    json.dump(spec(sys.argv[1]), open(sys.argv[2], "w"), indent=1)
