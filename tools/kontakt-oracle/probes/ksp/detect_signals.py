"""Write the WAVs probes/ksp/detect_laws.kspp detects (44.1 kHz, 16-bit, mono) into
/data/dev/workspace/BuildScratch/oracle-mf. The same signals are built by
kontra-host's tests (detections_follow_kontakts_laws)."""
import math, struct, sys, wave
from pathlib import Path

RATE = 44100
OUT = Path(sys.argv[1] if len(sys.argv) > 1 else "/data/dev/workspace/BuildScratch/oracle-mf")


def chord(freqs, seconds, amp=0.8):
    n = int(seconds * RATE)
    return [sum(math.sin(2 * math.pi * f * i / RATE) for f in freqs) * amp / len(freqs) for i in range(n)]


def write(name, xs):
    with wave.open(str(OUT / name), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(b"".join(struct.pack("<h", max(-32767, min(32767, round(x * 32767)))) for x in xs))


OUT.mkdir(parents=True, exist_ok=True)
write("c_major.wav", chord([261.63, 329.63, 392.0], 4.0))
write("a_minor.wav", chord([220.0, 261.63, 329.63], 4.0))
write("e_major.wav", chord([329.63, 415.30, 493.88], 2.5))
write("quiet.wav", chord([440.0], 1.0, amp=0.0005))
write("tone_750ms.wav", chord([440.0], 0.75, amp=0.5))
