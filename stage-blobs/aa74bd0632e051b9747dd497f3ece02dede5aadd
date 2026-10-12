# KSP millisecond listener disable sentinel

Static evidence, 2026-10-04, from the installed Kontakt 8.13.1 executable pinned
in [native controller evidence](../docs/CORE_NATIVE_CONTROLLER_EVIDENCE.md).
No native executable or original instrument was activated. Local captures are
retained under ignored `artifacts/core-native/listener/`.

The [NI primary manual](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/time-related-commands)
documents positive MS periods in microseconds and zero to disable a timer.
It does not document the reported initialization parameter `-1`.

## Constant and command identity

Native registration at `0x14046570f..0x140465732` binds
`$NI_SIGNAL_TIMER_MS` to symbol ID `0x4f0`; the next three symbols are BEAT
`0x4f1`, TRANSP_START `0x4f2`, and TRANSP_STOP `0x4f3`.
Scalar initializer `0x14077efc0` creates a table at `rsp+0x20`, with
`rbp=rsp+0x100`. Entry `0x4f0` is consequently at `rbp+0x12e0`.
`0x1407807c1` loads four signed integers from `0x144e906c0`: `(3,4,1,2)`;
`0x1407807db` stores them at that entry. The table is copied from `rsp+0x20`
with byte length `0x17c8` at `0x140780b56..0x140780c30`.
Thus the native MS signal value is 3. Kontakto uses a separate internal enum
whose MS value is 1; the screenshot's value 1 does not contradict this binding.

The AST construction branch at `0x1407abdb0` checks the init callback and
reports `set_listener() can only be used in "on init" callback!` on failure.
The accepted branch constructs opcode `0x17` at `0x1407abde6`.

## Signed parameter storage and timer consumer

The statement dispatcher evaluates both arguments at `0x14079d981` for
`set_listener`, and at `0x14079d8d6` for `change_listener_par`. Both branches
test native MS signal 3, and only convert the parameter when it is strictly
positive (`0x14079d9b1..0x14079d9c4` and `0x14079d906..0x14079d919`).
Nonpositive values pass through unchanged.

Registration helper `0x140995150` stores signal at listener record `+0x18`
and signed parameter at `+0x1c`. Update helper `0x1409826a0` writes the same
parameter field on the existing matching record.
The actual listener consumer at `0x140985a6f` selects signal 3, loads
`record+0x1c` at `0x140985a79`, compares against 1 at `0x140985a7c`, and
takes a signed less-than branch to the next record (`0x140985f93`).
Therefore `-1` is accepted and produces no MS timer callbacks in this native
path for both commands. This closes the disabled-timer interpretation of the
existing local `-1` to zero normalization. It does not establish every native
timer interval, quantization rule, or behavior for other signal types.

## Local regression boundary

`legacy_disabled_listener_initialization_and_timer_actions_reach_pcm` stages
a real Engine script: `-1` initialization preserves its performance UI and a
label written after registration; no ticks occur before frame 96. A note at
frame 96 enables a 1000-microsecond period at 48 kHz. Listener callbacks at
frames 144 and 192 change the voice gain; the second disables its timer with
`-1`. Complete stereo PCM is compared with independent engine gain changes
at those same frames, and callback counts exclude subsequent firings.
This is a focused reproduction of the reported failure path, not a load or
audio-equivalence claim for the unavailable library instruments. The test was
staged by the diagnostic agent; the coordinating agent owns its execution.
