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
  strcmp, and abort-on-throw stubs. Every other import returns 0 and is logged.

## Method notes

- Import slots: a slot is only replaced when it is the target of a rip-relative
  `call [rip+x]` / `jmp [rip+x]` in a code section and its value is an
  import-by-name RVA. This resolved 751 slots. Rodata string pointers are not
  touched.
- Virtual calls on synthetic objects: `fake_vtable(owner, slots, factory)`
  creates a vtable whose slots are named stubs (`owner::vslotN`). Handlers can
  return values (pointers, floats) and every call is logged with its
  register arguments.
- Stub returns: an integer return goes to `rax`, a float to `xmm0`.

## Scope limits found so far

- Knob pointer handler (vtable slot 3, `fcn.180739410`): runs to completion
  with an embedded child vtable at `this+0xda0` and a sub-object at
  `this+0x1020`. Filling every null pointer with fake objects is wrong: it
  sends the code down the RTTI branch at `0x180731d87`. Object graphs have to
  be built field by field.
- FFT wrapper (`0x180001640`): runs and returns `ippStsNoErr`. An impulse gives
  unscaled all-ones output for `N = 2^order`. Non-trivial inputs do not match a
  plain DFT, because the spec's twiddle tables are never built. The IPP init
  routine was not found near the wrapper.

## Reproduce

Scratch scripts live outside the repo (`RunScratch/smooth/re-emulate`). The
venv is `uv venv` on the dev drive with `unicorn`, `capstone`, `pefile`.
Run with `python -I`.

## Real-pipeline scripts

- `input_chain.py`: runs the wheel path with the real image code. Editor slot 8
  pushes into the ring, `try_pop` pops, and the UI consumer stores the wheel
  fields. It prints the verified values and the pcs of the stores. The MSVC
  clock stubs (`_Query_perf_frequency` 10 MHz, `_Query_perf_counter`) are the
  only synthetic inputs on the path.
