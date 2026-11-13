#!/usr/bin/env python3
"""Effect coverage probe: factory FX presets on one sample, one Kontakt session, each instance saved.

    coverage_probe.py stereo|spectral OUT_DIR

Renders OUT_DIR/kontakt.wav and saves OUT_DIR/<name>.nki per instance, so `kontra-inspect play`
renders the same instruments for ours. Preset state saves with the instrument (KSP init writes on
a preset-loaded effect do not), so no script is involved.
- stereo: L = 500 Hz, R = 1300 Hz, -12 dB each, 1.5 s; each output channel's L and R content
  reads apart by frequency (Stereo Modeller presets).
- spectral: a 0.5 s noise burst at -24 dB, then silence (EQ and convolution presets).
The first instance of a kind loaded in a session ramps in Kontakt (Stereo Modeller INIT: -4 dB,
left channel rising over 0.6 s), so each set starts with a throwaway instance.
"""
import json
import subprocess
import sys
from pathlib import Path

import numpy as np

KO = Path(__file__).resolve().parent
sys.path.insert(0, str(KO))
import eqrender  # noqa: E402
import fxrig  # noqa: E402

out = Path(sys.argv[2]).resolve()
out.mkdir(parents=True, exist_ok=True)
R = 48000
kind = sys.argv[1]
if kind == "stereo":
    t = np.arange(int(1.5 * R)) / R
    amp = 10 ** (-12 / 20)
    lr = np.stack([amp * np.sin(2 * np.pi * 500 * t), amp * np.sin(2 * np.pi * 1300 * t)], 1)
else:
    n = np.zeros(int(1.8 * R))
    n[: int(0.5 * R)] = np.random.default_rng(7).standard_normal(int(0.5 * R)) * 10 ** (-24 / 20)
    lr = np.stack([n, n], 1)
lr = lr.astype("<f4")
wav = out / "src.wav"
data = lr.tobytes()
wav.write_bytes(b"RIFF" + (36 + len(data)).to_bytes(4, "little") + b"WAVEfmt " +
                (16).to_bytes(4, "little") + (3).to_bytes(2, "little") + (2).to_bytes(2, "little") +
                R.to_bytes(4, "little") + (R * 8).to_bytes(4, "little") + (8).to_bytes(2, "little") +
                (32).to_bytes(2, "little") + b"data" + len(data).to_bytes(4, "little") + data)

P = "Effects/Stereo/"
SETS = {
    "stereo": [
        ("warm", [P + "Wide +50.nkp"], []),
        ("dry", [], []),
        ("st_init", [P + "- INIT Stereo Modeller -.nkp"], []),
        ("st_n100", [P + "Narrow -100.nkp"], []),
        ("st_n50", [P + "Narrow -50.nkp"], []),
        ("st_w50", [P + "Wide +50.nkp"], []),
        ("st_w100", [P + "Wide +100.nkp"], []),
    ],
    "spectral": [
        ("warm", [P + "Wide +50.nkp"], []),
        ("dry", [], []),
        ("conv_aud", ["Effects/Convolution/01 Real Rooms/Auditorium A.nkp"], []),
        ("conv_iso", ["Effects/Convolution/08 Tiny Rooms/0.1s Iso Booth.nkp"], []),
        ("eq3_radio", ["Filters/EQ/EQ3 - Old Radio.nkp"], []),
        ("eq2_hifi", ["Filters/EQ/EQ2 - Hi-Fi.nkp"], []),
        ("eq1_peak", ["Filters/EQ/EQ1 - Peak.nkp"], []),
    ],
}
INSTANCES = SETS[kind]

body = []
for k, (name, inserts, sends) in enumerate(INSTANCES):
    ins = ",".join(json.dumps(eqrender.winpath(eqrender.init_preset(p))) for p in inserts)
    snd = ",".join(json.dumps(eqrender.winpath(eqrender.init_preset(p))) for p in sends)
    body.append(f"""
do
  local i = kt.add_instrument()
  kt.set_instrument_midi_channel(i, {k + 1 + fxrig.BASE})
  local z = kt.add_zone(i, 0, WAV)
  kt.set_zone_geometry(i, z, {{ root_key = 60, low_key = 0, high_key = 127 }})
  for s, p in ipairs({{{ins}}}) do log('{{"ins":' .. tostring(pcall(kt.load_fx_preset, p, i, -1, s - 1, kt.insert_fx)) .. ',"k":{k}}}') end
  for s, p in ipairs({{{snd}}}) do
    local ok, e = pcall(kt.load_fx_preset, p, i, -1, s - 1, kt.send_fx)
    log('{{"send":' .. tostring(ok) .. ',"e":"' .. tostring(e):gsub('"', "'") .. '","k":{k}}}')
  end
  local ok, e = pcall(kt.save_instrument, i, {json.dumps(eqrender.winpath(out / (name + ".nki")))}, {{}})
  log('{{"save":' .. tostring(ok) .. ',"e":"' .. tostring(e):gsub('"', "'") .. '","k":{k}}}')
end""")
lua = ("local kt = Kontakt\nlocal out = assert(io.open(ORACLE_OUT, \"w\"))\n"
       "local function log(t) out:write(t .. \"\\n\") out:flush() end\n"
       f"local WAV = {json.dumps(eqrender.winpath(wav))}\nkt.reset_multi()\n" + "".join(body) +
       "\nlog('{\"ready\":true}')\nout:close()\n")
(out / "probe.lua").write_text(lua)
fxrig.SLOT = 2.0 if kind == "stereo" else 4.0
fxrig.write_midi(len(INSTANCES), out / "probe.mid", hold=1.6 if kind == "stereo" else 0.6)
total = fxrig.FIRST + len(INSTANCES) * fxrig.SLOT + 2.0
subprocess.run([sys.executable, str(KO / "render.py"), str(out / "probe.lua"), str(out / "kontakt.wav"),
                str(total), str(out / "probe.mid")], check=True)
(out / "instances.json").write_text(json.dumps([n for n, _, _ in INSTANCES]))
