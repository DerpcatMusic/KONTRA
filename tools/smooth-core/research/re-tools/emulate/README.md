# soothe3 isolated-function emulation

Runs single functions of the restored soothe3 PE under Unicorn (x86-64).
This is not a Windows process: it shows what a function computes for the
inputs we construct. It is evidence for the function's own behaviour, not for
the plug-in in a host.

## Files

- `pe_emu.py`: loader and call helper. Maps the image at `0x180000000`,
  resolves the import directory and the IAT-like table in `.rdata`, and
  provides a Win64 `call()`, memory write/read watches, an optional
  instruction trace, and `abort()` for throw paths.
- `crt.py`: real handlers for malloc/free, HeapAlloc, memcpy/memset/memcmp,
  strcmp, and abort-on-throw stubs.
- `cpu.py`: CPUID profile (reference-class Intel, SSE3 set). Assumption, not a
  measured value.
- `libm.py`: real `pow`/`exp`/`log`/trig/rounding imports. A float-returning
  import must not use the default stub (it returns 0 and leaves XMM0 alone).
- `fake_host.py`: runs the DSP constructor `0x88fa90` against a synthetic host
  (registration slot +0x18, getter +0x20).
- `detector_probe.py`: diagnostics for `0x619fc0`: kernel inputs, output against amplitude and group count,
  output against a flat weight. Section 16. Run with `python -I detector_probe.py` (about 70 s).
- `detector_run.py`: runs the reduction detector `0x619fc0` on a stand-in host state. It fills the
  bins and weights through `0x621d50`, then reports the first fault with its function and the
  registers. Stand-in state only; see `../../re-latency-fft-scale.md` section 15.
- `oracle.py`: constructor-state oracle. Runs the constructor per sample rate
  and calls `getLatency` (`0x88f610`). Writes JSON to
  `tools/smooth-core/research/oracle/`.

## Oracle results (constructor state, step 1)

See `../oracle/ctor_latency_mode4.json` and `../oracle/ctor_latency_mode1.json`.
Scope: constructor state only. No process path has run, so these are not impulse
delays and not output levels.

- `groups = max(1, trunc(rate / 44100))`, checked at 44.1, 48, 64, 88.2, 96,
  110.25, 132.3, 176.4 and 192 kHz.
- `vec_count = 48 * groups`.
- `getLatency = groups * H + base`, with `H = 2304` (fields `+0x2c78` = 2048 and
  `+0x30` = 256, constant across rates).
- `base = 48 * groups` when `groups >= 2`, and `0` when `groups == 1`. Observed
  0, 96, 144, 192 for groups 1..4. The rule is fitted on these four group
  counts (the rate points are 88.2/96/110.25, 132.3, and 176.4/192), not derived
  from the code, so a group count of 5+ is untested.
- Analysis mode 1 (CPU flag bit 0 clear) gives the same latency as mode 4.
  Mode 1 was checked at 44.1, 96 and 192 kHz only.

### Entry 14 anomaly: resolved

The constructor writes group-0 entry 14 as a **double** (8 bytes at `+0xb7e0`),
not a float. Reading that slot as a float gave 2.7e22 and -0.0. The double is
about `33.3 / rate`: 7.556e-4 at 44.1 kHz, 6.942e-4 at 48 kHz, 3.472e-4 at
96 kHz, 1.736e-4 at 192 kHz (mode 1 at 96/192 matches the same law). This is a
rate-dependent coefficient, not garbage. Its exact form is not derived.

### Caution on `getLatency`

`getLatency` reads `X = [rcx]`, so its argument is a pointer to the object
pointer. Passing the object directly reads the wrong bytes and returns
`0x40000000`. `oracle.py` does this correctly (`get_latency`).

## Not done

- The detection routine `0x88c5f0` is not run. The mode is set through the
  writer `0x30d310` with the CPU flag, which reproduces the constructor state.
- Latency for the other config enums (quality, size, low-latency mode) is not
  measured; the constructor was run with a zero config only.
- The process path (`0x890a00`, `0x180619fc0`) is not run. The detector needs host-built
  state (std::map lookups at `S+0x538/0x550` and the bin/weight vectors). See
  `../re-latency-fft-scale.md` section 14 for the fault chain.

## FFT primitives (verified, `fft_verify.py`)

The FFT wrappers DO run under emulation, once the init thunk gets its six arguments (see
`fft_verify.py` docstring). `oracle/fft_verify.json` records the check against numpy at orders
1..12 (worst relative error 1.9e-7) and each primitive. No numpy hook is needed.

Corrections to the earlier notes: `0x1280` is the unscaled real INVERSE (not a forward), `0x1010`
is the real forward, and `0x6181b0` returns the magnitude `|Y|`, not the power. Run with:

    python -I fft_verify.py ../../oracle/fft_verify.json

## Reproduce

The venv is `uv venv` on the dev drive with `unicorn`, `capstone`, `pefile`
(`/data/dev/workspace/RunScratch/smooth/re-emulate/.venv`). Run with `-I`:

    python -I oracle.py ../oracle/ctor_latency_mode4.json --mode 4
    python -I oracle.py ../oracle/ctor_latency_mode1.json --mode 1 --rates 44100,96000,192000

## Real-pipeline scripts

- `input_chain.py`: runs the wheel path with the real image code. Editor slot 8
  pushes into the ring, `try_pop` pops, and the UI consumer stores the wheel
  fields. It prints the verified values and the pcs of the stores. The MSVC
  clock stubs (`_Query_perf_frequency` 10 MHz, `_Query_perf_counter`) are the
  only synthetic inputs on the path.

## Process path (section 17 of ../../re-latency-fft-scale.md)

- `disasm_rva.py`: disassembles an RVA range through the emulator loader, naming IAT imports and
  corpus functions. `python -I disasm_rva.py 0x890a00 0x31d7`.
- `process_lib.py`: `Session(rate)` builds the constructor state, wraps it the way the host holds it
  (wrapper slots +0x10 process thunk, +0x18 prepare, +0x28 getLatency), fills typed host values
  (ids 10 and 12 are floats; without them the ring buffers have no channels), and provides
  `add_probe` (per-PC hooks) and `fault_report` (fault PC, function, disassembly, registers, stack
  return addresses).
- `process_oracle.py`: runs the process block (entry `0x890a00`) for K blocks at 44.1, 48, 96 and
  192 kHz in two latency modes, measures the impulse delay, the noise cross-correlation delay, the
  sine level at -40, -20 and -6 dBFS (48 kHz), and whether `0x619fc0` executes. Writes
  `../../oracle/process_block_<rate>.json`. Run with `python -I process_oracle.py` (several minutes).
  Use `--rates 48000` for a quick run.
- Pitfall: `Emulator.reg(0)` is the invalid register id (returns 0 with a warning). Use
  `UC_X86_REG_RAX` and friends.
- Note on `ctor_latency_mode{4,1}.json`: written by `oracle.py` with the host's zero defaults (the
  constructor's ring buffers then had no channels). Re-checked with the typed host of `fake_host.run_ctor`
  (layout 2.0, samplerate = rate): latency is the same at 44.1, 48, 96 and 192 kHz. Section 17.1.
- Exact commands for the process oracle (venv at `/data/dev/workspace/RunScratch/smooth/re-emulate/.venv`):
  `python -I process_oracle.py --rates 44100,96000,192000` (about 4 minutes each rate pair) and
  `python -I process_oracle.py --rates 48000` (levels and noise, about 4 minutes), then
  `python -I detector_reach.py` (about 1 minute).
