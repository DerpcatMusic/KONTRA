#!/usr/bin/env python3
"""Kontakt's own load time and memory per instrument (the RAM/load oracle).

    tools/oracle/oracle-slot python3 tools/kontakt-oracle/memprobe.py A.nki [B.nki ...]

Runs dump.py with a probe that loads one instrument and returns, leaving Kontakt's main
loop free to finish its background loading, while this script samples the Kontakt
process's RSS for MEMPROBE_SECONDS (60). Prints the load_instrument time and the RSS
before the load, at its peak and at the end. One instrument per run: Kontakt does not
give memory back on reset_multi.
"""
import os
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "oracle"))
import slot  # noqa: E402

PROBE = r"""
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function mark(s) out:write(s, "\n") out:flush() end
local function idle(s) local t = os.clock() while os.clock() - t < s do end end
pcall(kt.reset_multi)
idle(4)
mark('{"phase":"load"}')
local t = os.clock()
local ok = pcall(kt.load_instrument, ORACLE_INPUTS[1])
mark('{"phase":"loaded","ok":' .. tostring(ok) .. ',"load_s":' .. (os.clock() - t) .. '}')
out:close()
"""


def rss_kb(pids):
    total = 0
    for p in pids:
        try:
            for line in Path(f"/proc/{p}/status").read_text().splitlines():
                if line.startswith("VmRSS:"):
                    total += int(line.split()[1])
        except OSError:
            pass
    return total


def read_mb(pids):
    """Bytes the processes read through read() calls (rchar), MB: disk and cache alike."""
    total = 0
    for p in pids:
        try:
            total += int(next(l for l in Path(f"/proc/{p}/io").read_text().splitlines() if l.startswith("rchar:")).split()[1])
        except (OSError, StopIteration):
            pass
    return total / 1e6


def main() -> int:
    inputs = sys.argv[1:2]
    seconds = float(os.environ.get("MEMPROBE_SECONDS", 60))
    probe = Path(os.environ.get("TMPDIR", "/tmp")) / f"memprobe-{os.getpid()}.lua"
    probe.write_text(PROBE)
    child = subprocess.Popen([sys.executable, str(HERE / "dump.py"), str(probe.with_suffix(".jsonl")), *inputs],
                             env=dict(os.environ, ORACLE_LUA=str(probe), ORACLE_TIMEOUT=str(seconds)))
    raw = slot.work(Path.home() / ".cache/kontra-native-oracle") / "run" / str(child.pid) / "oracle.jsonl"
    samples = []  # (phase line count, rss kB, last line)
    reads = []
    while child.poll() is None:
        lines = raw.read_text(errors="replace").splitlines() if raw.exists() else []
        pids = slot.kontakt_pids()
        samples.append((len(lines), rss_kb(pids), lines[-1] if lines else ""))
        reads.append(read_mb(pids) if pids else (reads[-1] if reads else 0.0))
        time.sleep(0.25)
    probe.unlink(missing_ok=True)
    base = [kb for n, kb, _ in samples if n == 0 and kb]
    after = [(kb, last) for n, kb, last in samples if n >= 1 and kb]
    if not base or not after:
        print(f"{Path(inputs[0]).name}: no samples")
        return 1
    peak, (end, last) = max(kb for kb, _ in after), after[-1]
    first = next(i for i, (n, _, _) in enumerate(samples) if n >= 1)
    read = max(reads) - reads[first - 1]
    print(f"{Path(inputs[0]).name}: {last}  RSS before {base[-1] / 1024:.0f} MB, "
          f"peak +{(peak - base[-1]) / 1024:.0f} MB, end +{(end - base[-1]) / 1024:.0f} MB "
          f"({len(after) / 4:.0f} s after the load began), read {read:.0f} MB")
    return 0


if __name__ == "__main__":
    sys.exit(main())
