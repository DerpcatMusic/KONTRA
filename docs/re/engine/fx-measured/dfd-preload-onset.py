# scratch: first-onset per note from captured slot WAVs (note-on at 0.2/1.6/3.0 s after slot start, slot at 0.5 s in WAV)
import sys, json
sys.path.insert(0, "/data/dev/workspace/worktrees/KONTRA/measure-dfd-preload/tools/kontakt-oracle")
import numpy as np, wavutil
W = sys.argv[1]
NOTES = [0.2, 1.6, 3.0]
PRE = 0.5
def first_onset(x, rate, t0, db):
    sus = x[int((t0 + 0.2) * rate):int((t0 + 0.4) * rate)]
    ref = np.sqrt(np.mean(sus ** 2)) + 1e-12
    fr = int(0.005 * rate)
    seg = x[int(t0 * rate) - int(0.05 * rate): int((t0 + 0.6) * rate)]
    n = len(seg) // fr
    rms = np.sqrt(np.mean(seg[: n * fr].reshape(n, fr) ** 2, axis=1))
    hit = np.nonzero(rms >= ref * 10 ** (db / 20))[0]
    if not len(hit):
        return None
    return (hit[0] * fr + 0.0) / rate - 0.05
out = []
for name in sys.argv[2:]:
    ch, rate, chans = wavutil.read(f"{W}/{name}.wav")
    x = np.mean(np.array(chans), axis=0)
    for k, n in enumerate(NOTES):
        t0 = PRE + n
        r = {"slot": name, "note": k + 1}
        for db in (-20, -30):
            v = first_onset(x, rate, t0, db)
            r[f"onset_ms_{-db}dB"] = None if v is None else round(v * 1000, 1)
        out.append(r)
for r in out:
    print("\t".join(str(r[k]) for k in ("slot", "note", "onset_ms_20dB", "onset_ms_30dB")))
