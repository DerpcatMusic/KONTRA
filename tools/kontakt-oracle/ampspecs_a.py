#!/usr/bin/env python3
"""Write stimrender.py specs for the Kontakt amp preamps this unit owns (k-amps-a).

    ampspecs_a.py bassinvader|basspro|hotsolo|epp OUT.json

Same format as ampspecs.py: each knob alone at low/mid/high (booleans off/on) over the `ramp`
stimulus (200 Hz, 0 -> 1.0), other knobs at mid. Names are the $ENGINE_PAR_* suffixes from
docs/re/kontakt-bindings/fx_other.tsv (HotSolo, Bass Invader, Bass Pro) and the engine parameter
list (EP Preamps has no binding rows yet, so its names are the manual's controls).
"""
import json
import sys

M = 1000000

MODULES = {
    "bassinvader": {
        "chain": ["Effects/Bass Invader"],
        "knobs": [("BASSINVADER_VOLUME", "f"), ("BASSINVADER_TREBLE", "f"), ("BASSINVADER_HI_MID", "f"),
                  ("BASSINVADER_LO_MID", "f"), ("BASSINVADER_BASS", "f"), ("BASSINVADER_BOOST", "f"),
                  ("BASSINVADER_MASTER", "f"), ("BASSINVADER_HI_BOOST", "b"), ("BASSINVADER_MID_CONTOUR", "b"),
                  ("BASSINVADER_LO_CUT", "b")],
    },
    "basspro": {
        "chain": ["Effects/Bass Pro"],
        "knobs": [("BASSPRO_GAIN", "f"), ("BASSPRO_BASS", "f"), ("BASSPRO_MID", "f"), ("BASSPRO_MIDFREQ", "f"),
                  ("BASSPRO_TREBLE", "f"), ("BASSPRO_MASTER", "f"),
                  ("BASSPRO_GEQ_VOLUME", "f"), ("BASSPRO_BRIGHT", "b"), ("BASSPRO_ULTRALO", "b"),
                  ("BASSPRO_ULTRAHI", "b"), ("BASSPRO_GEQ", "b"), ("BASSPRO_MONO", "b"), ("BASSPRO_DRIVE", "f")],
    },
    "hotsolo": {
        "chain": ["Effects/HotSolo"],
        "knobs": [("HS_PRENORMAL", "f"), ("HS_PRESENCE", "f"),
                  ("HS_DEPTH", "f"), ("HS_TREBLE", "f"), ("HS_MID", "f"), ("HS_BASS", "f"),
                  ("HS_OVERDRIVE", "b"), ("HS_MONO", "b"), ("HS_PREOVERDRIVE", "f"), ("HS_MASTER", "f")],
    },
    "epp": {
        "chain": ["Effects/EP Preamps"],
        "knobs": [("EPP_DRIVE", "f"), ("EPP_EQ_BASS", "f"), ("EPP_EQ_MID", "f"), ("EPP_EQ_TREBLE", "f"),
                  ("EPP_PASSIVE_BASS", "f"), ("EPP_MONO", "b")],
    },
}


# Knobs that silence the chain at 0 (DRIVE, PREOVERDRIVE, MASTER) run last: a 0 that goes silent sticks
# for the rest of the session, which would hide every later step.

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
