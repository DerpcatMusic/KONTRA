#!/usr/bin/env python3
"""How Kontakt 8.13 delivers RPN and NRPN (KSP `on rpn`, `on nrpn`, $RPN_ADDRESS, $RPN_VALUE),
read back through the audio (an earlier harness that saved arrays from the callbacks wrote nothing).

    tools/oracle/oracle-slot python3 tools/kontakt-oracle/probes/rpn_audio.py

One instrument holds a 440 Hz tone over every key. Each callback queues what it read as notes,
and a 120 ms timer listener plays them one at a time, so callbacks never overlap:
- a marker key: 101 rpn, 103 nrpn, 105 controller, 107 note;
- then base-16 digits of each value, high digit first, as keys 84 + digit.
  rpn and nrpn send 4 digits of $RPN_ADDRESS and 4 of $RPN_VALUE; a controller sends 2 of $CC_NUM
  and 2 of %CC[$CC_NUM]; a note sends 2 of $EVENT_NOTE.
A note is where one key's level rises in the recording. The opening note (key 60) proves the
harness: it must come back as `note 60`.

Measured 2026-11-25 (Kontakt 8.13.1), in callback order (one stray digit after `controller 99`):
  note 60 | controller 101 0, 100 0, 6 2 | rpn 0 256 | controller 38 0
  controller 101 0, 100 1, 6 64 | rpn 1 8202 | controller 38 10
  controller 99 1, 98 5, 6 3 | nrpn 133 391 | controller 38 7
  controller 6 9 (no rpn) | controller 96 0 (no rpn)
So on rpn / on nrpn run when the data entry LSB (CC 38) arrives, before that CC's on controller:
$RPN_ADDRESS = CC101 * 128 + CC100 (nrpn: CC99, CC98), $RPN_VALUE = CC6 * 128 + CC38. A data entry
MSB alone, or an increment, runs only on controller.
"""
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
sys.path.insert(0, str(HERE.parent.parent / "oracle"))
import eqrender  # noqa: E402
import numpy as np  # noqa: E402
import slot  # noqa: E402

TONE = Path("/data/dev/workspace/BuildScratch/oracle-mf/tone.wav")  # probes/ksp/user_zones_sample.kspp
TICK = 0.12
# (label, [(cc, value)...]) sent 50 ms apart; each step gets time for its notes to play.
STEPS = [
    ("rpn 0/0 data 2/0", [(101, 0), (100, 0), (6, 2), (38, 0)]),
    ("rpn 0/1 data 64/10", [(101, 0), (100, 1), (6, 64), (38, 10)]),
    ("nrpn 1/5 data 3/7", [(99, 1), (98, 5), (6, 3), (38, 7)]),
    ("data msb 9 alone", [(6, 9)]),
    ("increment", [(96, 0)]),
]
MARK = {101: "rpn", 103: "nrpn", 105: "controller", 107: "note"}
DIGITS = {"rpn": 8, "nrpn": 8, "controller": 4, "note": 2}
KSP = """on init
  declare %q[2000]
  declare $w := 0
  declare $r := 0
  declare $i
  declare $v
  declare $n
  set_listener($NI_SIGNAL_TIMER_MS, TICK_US)
end on
function push
  $i := $n - 1
  while ($i >= 0)
    %q[$w] := 84 + ($v / sh_left(1, 4 * $i)) mod 16
    $w := $w + 1
    $i := $i - 1
  end while
end function
on listener
  if ($r < $w)
    play_note(%q[$r], 100, 0, 50000)
    $r := $r + 1
  end if
end on
on note
  ignore_event($EVENT_ID)
  %q[$w] := 107
  $w := $w + 1
  $v := $EVENT_NOTE
  $n := 2
  call push
end on
on rpn
  %q[$w] := 101
  $w := $w + 1
  $v := $RPN_ADDRESS
  $n := 4
  call push
  $v := $RPN_VALUE
  $n := 4
  call push
end on
on nrpn
  %q[$w] := 103
  $w := $w + 1
  $v := $RPN_ADDRESS
  $n := 4
  call push
  $v := $RPN_VALUE
  $n := 4
  call push
end on
on controller
  %q[$w] := 105
  $w := $w + 1
  $v := $CC_NUM
  $n := 2
  call push
  $v := %CC[$CC_NUM]
  $n := 2
  call push
end on
""".replace("TICK_US", str(round(TICK * 1e6)))


def midi(path: Path, ch: int) -> float:
    ev, t = [(0.3, bytes([0x90 | ch, 60, 100])), (0.4, bytes([0x80 | ch, 60, 0]))], 1.0
    for _, ccs in STEPS:
        for cc, v in ccs:
            ev.append((t, bytes([0xB0 | ch, cc, v])))
            t += 0.05
        t += 0.5
    trk, last = b"\x00\xff\x51\x03" + (500000).to_bytes(3, "big"), 0
    for at, m in ev:
        tick = round(at * 960)
        trk += eqrender.vlq(tick - last) + m
        last = tick
    trk += b"\x00\xff\x2f\x00"
    head = b"MThd" + (6).to_bytes(4, "big") + bytes([0, 0, 0, 1]) + (480).to_bytes(2, "big")
    path.write_bytes(head + b"MTrk" + len(trk).to_bytes(4, "big") + trk)
    # Every CC can queue a controller (5 notes) and an rpn (9): budget for all of them.
    notes = 3 + sum(len(c) * 14 for _, c in STEPS)
    return max(t, notes * TICK) + 3.0


def keys(wav: Path) -> list[int]:
    """The key of every note in a 48 kHz float stereo recording, in order. Notes ring on (the
    tone is 1 s long), so a note is where one key's level rises, not where the level does."""
    raw = wav.read_bytes()
    data = raw[raw.find(b"data") + 8:] if raw[:4] == b"RIFF" else raw
    x = np.frombuffer(data[: len(data) // 8 * 8], dtype="<f4").reshape(-1, 2).sum(1).astype(float)
    rate, hop, n = 48000, 480, 4096
    ks = np.arange(55, 112)
    t = np.arange(n) / rate
    basis = np.exp(-2j * np.pi * (440 * 2 ** ((ks - 69) / 12))[:, None] * t[None, :]) * np.hanning(n)
    level = np.array([np.abs(basis @ x[i:i + n]) for i in range(0, len(x) - n, hop)])
    rise = np.maximum(0, level[2:] - level[:-2])
    out, last = [], {}
    for f, r in enumerate(rise):
        k = int(np.argmax(r))
        if r[k] > 0.25 * rise.max() and f - last.get(k, -100) > 6 and (not out or f - out[-1][0] > 3):
            out.append((f, int(ks[k])))
            last[k] = f
    return [k for _, k in out]


def decode(ks: list[int]) -> list[str]:
    out, i = [], 0
    while i < len(ks):
        kind = MARK.get(ks[i])
        if not kind:
            out.append(f"? {ks[i]}")
            i += 1
            continue
        digits = ks[i + 1:i + 1 + DIGITS[kind]]
        vals, half = [], len(digits) // (1 if kind == "note" else 2)
        for j in range(0, len(digits), half):
            vals.append(sum((d - 84) << (4 * (half - 1 - m)) for m, d in enumerate(digits[j:j + half])))
        out.append(" ".join([kind] + [str(v) for v in vals]))
        i += 1 + DIGITS[kind]
    return out


def main():
    work = eqrender.scratch_dir()
    seconds = midi(work / "rpn.mid", slot.channel_block()[0])
    ksp = KSP.replace("\\", "\\\\")
    lua = work / "rpn.lua"
    lua.write_text("\n".join([
        "local kt = Kontakt",
        'local out = assert(io.open(ORACLE_OUT, "w"))',
        "kt.reset_multi()",
        "local i = kt.add_instrument()",
        "kt.set_instrument_midi_channel(i, ORACLE_CHANNEL or 1)",
        f"local z = kt.add_zone(i, 0, {eqrender.json.dumps(eqrender.winpath(TONE))})",
        "kt.set_zone_geometry(i, z, { root_key = 69, low_key = 0, high_key = 127 })",
        "local ok, err = pcall(kt.set_instrument_script_source, i, 0, [==[" + ksp + "]==])",
        'out:write(string.format(\'{"compiled":%s}\\n\', tostring(ok)))',
        'out:write(\'{"ready":true}\\n\')',
        "out:close()",
    ]) + "\n")
    wav = work / "rpn.wav"
    subprocess.run([sys.executable, str(HERE.parent / "render.py"), str(lua), str(wav), str(seconds),
                    str(work / "rpn.mid")], check=True)
    ks = keys(wav)
    print("keys:", ks)
    for line in decode(ks):
        print(line)


if __name__ == "__main__":
    main()
