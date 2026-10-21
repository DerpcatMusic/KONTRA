"""Process-path oracle: impulse delay, level and signal dependence of block 0x890a00.

Runs the real process block (entry 0x890a00, reached through the wrapper thunk 0x88f4f0)
on the constructor state from process_lib.Session, for several sample rates and two
latency modes. Writes oracle/process_block_<rate>.json (one file per rate).

Modes (both read by the block and by getLatency 0x88f610, see section 17):
  mode 0  default stand-in: [obj+0x237]=0, [obj+0xab]=0. Reports groups*2304+base.
  mode 1  zero-latency: [obj+0x237]=1 (host flag 3 through wrapper slot +0x50).
          getLatency returns base only, and the block takes the zero-latency convolver.

Harness conventions (verified in section 17):
  process(this, out=rdx, in=r8, side=r9 (null -> frame built from r8), N=5th arg).
  Output is written in place into the rdx tables. Input is read from the r8 tables.

Evidence scope: isolated emulation with a synthetic host. Stand-ins: 2 channels (layout 2),
block size N=512, host bool isset=0 for every id, all band and output parameters at the
constructor's zero state (no parameters set by a host). A stand-in band state is the identity,
so level and delay results describe the pass-through path, not any EQ or reduction gain.

Run with: python -I process_oracle.py [--rates 44100,48000,96000,192000] [--out-dir DIR]
"""

import argparse
import json
import math
import random
import struct
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from process_lib import (  # noqa: E402
    PROCESS_THUNK,
    Session,
    alloc_frames,
    fault_report,
    read_frames,
)
from pe_emu import IMAGE_BASE  # noqa: E402

N = 512  # block size, stand-in
K = 26  # blocks per run: covers the 9408-sample latency at 192 kHz plus a 4096-sample window
ORACLE_DIR = Path(__file__).resolve().parents[2] / "oracle"
DETECTOR_RANGE = (0x619FC0, 0x61A000)  # 0x619fc0 (reduction detector) and its first 64 bytes
ZL_OFFSET = 0x237  # zero-latency flag byte, set by wrapper slot +0x50 with which=3
TRACE_BLOCK = 0  # the block whose executed PCs are recorded


def run_blocks(sess, in_ch, out_ch_len, trace=False):
    """Process K blocks of the input channels. Returns (out0, out1, executed_rva_set, fault)."""
    emu = sess.emu
    outs = [[], []]
    executed = None
    fault = None
    total = K * N
    for k in range(K):
        blk = []
        for c in range(2):
            seg = in_ch[c][k * N:(k + 1) * N]
            blk.append(seg)
        tin, _ = alloc_frames(emu, 2, N, blk)
        tout, bufs_out = alloc_frames(emu, 2, N, [[0.0] * N, [0.0] * N])
        pcs = None
        if trace and k == TRACE_BLOCK:
            pcs = set()
            emu.trace_insns(pcs)
        try:
            emu.call(PROCESS_THUNK, sess.wrapper, tout, tin, 0, N, max_insns=400_000_000)
        except RuntimeError as exc:
            fault = fault_report(emu, exc)
            break
        finally:
            if pcs is not None:
                emu.insn_trace = None
                executed = {p - IMAGE_BASE for p in pcs}
        frames = read_frames(emu, bufs_out, N)
        outs[0] += frames[0]
        outs[1] += frames[1]
    return outs[0], outs[1], executed, fault


def impulse_delay(y):
    peak = max(range(len(y)), key=lambda i: abs(y[i]))
    nonzero = sum(1 for v in y if abs(v) > 1e-7)
    return peak, y[peak], nonzero


def noise_delay(y, x, max_lag=3000, win=2048):
    """Lag L maximising sum_t y[t]*x[t-L] over a window after the start (lags 0..max_lag)."""
    start = max_lag + 2048
    ys = y[start:start + win]
    best = (0, -1e30)
    for lag in range(max_lag + 1):
        xs = x[start - lag:start - lag + win]
        c = sum(a * b for a, b in zip(ys, xs))
        if c > best[1]:
            best = (lag, c)
    return best[0]


def sine(rate, amp, total):
    return [amp * math.sin(2 * math.pi * 1000.0 * i / rate) for i in range(total)]


def rms(v):
    return math.sqrt(sum(x * x for x in v) / len(v))


def run_rate(rate, mode, with_levels, with_noise, log):
    sess = Session(rate)
    sess.prepare()
    if mode == 1:
        sess.emu.wr(sess.obj + ZL_OFFSET, b"\x01")
    lat = sess.latency()
    total = K * N
    row = {"rate": rate, "mode": mode, "latency_getter": lat, "groups": struct.unpack(
        "<I", sess.emu.rd(sess.obj + 0x2C, 4))[0], "block": N, "blocks": K}
    imp = [0.0] * total
    imp[0] = 1.0
    t0 = time.time()
    y0, y1, executed, fault = run_blocks(sess, [imp, imp], total, trace=True)
    if fault:
        row["fault"] = fault
        log.append(f"rate {rate} mode {mode}: FAULT\n{fault}")
        return row
    d0, v0, nz0 = impulse_delay(y0)
    d1, v1, nz1 = impulse_delay(y1)
    row.update({"impulse_delay_ch0": d0, "impulse_value_ch0": v0, "nonzero_ch0": nz0,
                "impulse_delay_ch1": d1, "impulse_value_ch1": v1, "nonzero_ch1": nz1,
                "run_seconds": round(time.time() - t0, 2)})
    lo, hi = DETECTOR_RANGE
    row["detector_0x619fc0_executed"] = any(lo <= p < hi for p in executed)
    row["executed_pcs_block0"] = len(executed)
    if with_noise:
        rnd = random.Random(12345)
        noise = [rnd.uniform(-0.5, 0.5) for _ in range(total)]
        n0, _, _, nfault = run_blocks(sess, [noise, noise], total)
        if nfault:
            row["noise_fault"] = nfault
        else:
            row["noise_delay_samples"] = noise_delay(n0, noise)
            row["noise_delay_note"] = "lag maximising the cross-correlation of output with input"
    if with_levels:
        levels = []
        for db in (-40.0, -20.0, -6.0):
            amp = 10 ** (db / 20)
            x = sine(rate, amp, total)
            s0, _, _, sfault = run_blocks(sess, [x, x], total)
            if sfault:
                levels.append({"dbfs": db, "fault": sfault})
                continue
            d = d0  # delay from the impulse run
            start = d + 4096
            win = 2048
            yo = s0[start:start + win]
            xi = x[start - d:start - d + win]
            g = rms(yo) / rms(xi)
            err = max(abs(a - b) for a, b in zip(yo, xi)) / amp
            levels.append({"dbfs": db, "gain_db": round(20 * math.log10(g), 4),
                           "max_residual_rel": err, "delay_used": d})
        row["levels"] = levels
    return row


def main(argv):
    ap = argparse.ArgumentParser()
    ap.add_argument("--rates", default="44100,48000,96000,192000")
    ap.add_argument("--out-dir", default=str(ORACLE_DIR))
    args = ap.parse_args(argv)
    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    log = []
    for r in (float(x) for x in args.rates.split(",")):
        rows = []
        for mode in (0, 1):
            rows.append(run_rate(r, mode, with_levels=(r == 48000.0), with_noise=(r == 48000.0), log=log))
        doc = {
            "source": "process block 0x890a00 under Unicorn, constructor state, synthetic host",
            "evidence_scope": "isolated emulation; stand-ins listed in process_oracle.py docstring",
            "block_size": N,
            "blocks": K,
            "rows": rows,
        }
        path = out_dir / f"process_block_{int(r)}.json"
        path.write_text(json.dumps(doc, indent=2) + "\n")
        # Check: the measured impulse delay equals the getLatency the constructor reports, in both modes.
        # ponytail: a check on the stand-in state only; it does not validate the plug-in default.
        for row in rows:
            if "fault" not in row:
                assert row["impulse_delay_ch0"] == row["latency_getter"] == row["impulse_delay_ch1"], row
        print(path, [(row["mode"], row.get("impulse_delay_ch0"), row.get("latency_getter"),
                      row.get("detector_0x619fc0_executed")) for row in rows])
    for line in log:
        print(line)


if __name__ == "__main__":
    main(sys.argv[1:])
