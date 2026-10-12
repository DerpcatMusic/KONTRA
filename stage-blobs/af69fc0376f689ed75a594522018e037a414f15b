#!/usr/bin/env python3
"""Prepare owned input bytes only. No plugin, host, CLI, library or audio job."""
import hashlib
import json
from pathlib import Path
import struct
import wave

ROOT = Path(__file__).resolve().parents[2]
DEST = ROOT / '.cache/pi-perf-round5-inputs'


def keep(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists():
        assert path.read_bytes() == data, 'do not replace changed input: ' + str(path)
    else:
        path.write_bytes(data)
    return {'path': str(path), 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}


# Integer triangle, period 200 frames, stereo PCM16, 8 seconds at 48 kHz.
period = b''.join(struct.pack('<hh', x, x) for i in range(200)
                  for x in [(-4096 + (8192 * i // 100)) if i < 100
                            else (4096 - (8192 * (i - 100) // 100))])
pcm = period * 1920
header = (b'RIFF' + struct.pack('<I', 36 + len(pcm)) + b'WAVEfmt '
          + struct.pack('<IHHIIHH', 16, 1, 2, 48000, 192000, 4, 16)
          + b'data' + struct.pack('<I', len(pcm)))
tone = DEST / 'samples/tone.wav'
files = [keep(tone, header + pcm)]
with wave.open(str(tone), 'rb') as w:
    assert (w.getnchannels(), w.getsampwidth(), w.getframerate(), w.getnframes()) == (2, 2, 48000, 384000)
    assert w.readframes(384000) == pcm
assert min(struct.unpack('<400h', period)) == -4096
assert max(struct.unpack('<400h', period)) == 4096
part = dict(path=str(tone), program=0, port=0, channel=-1, output=0,
            output_manual=True, gain=0.0, aux=-1, aux_gain=-60.0, mic_buses=[], mic_names=[])
multi = dict(format='kontra-multi', version=2, name='Owned synthetic warm CLAP', parts=[part])
files.append(keep(DEST / 'input.kontra-multi', (json.dumps(multi, sort_keys=True, separators=(',', ':')) + '\n').encode()))
events = [(0, 0xb0, 1, 110), (0, 0xb0, 11, 127), (0, 0xb0, 64, 127)]
for key in range(48, 60):
    events.extend([(0, 0x90, key, 100), (48000, 0x80, key, 0)])
events.append((144000, 0xb0, 64, 0))
events.sort(key=lambda e: e[0])
assert len(events) == 28 and [e[2] for e in events if e[1] == 0x90] == list(range(48, 60))
files.append(keep(DEST / 'events.tsv', ''.join('\t'.join(map(str, e)) + '\n' for e in events).encode()))
print(json.dumps({'scope': 'authored input bytes only, no runtime proof', 'files': files}, indent=2))
