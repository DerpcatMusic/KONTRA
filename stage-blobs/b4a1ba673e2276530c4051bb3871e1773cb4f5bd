# Native voice-chain cold startup hold

Implementation: `0af58a6d`; fixture admission correction: `78af0703`.
Integration merge `3704cbcc` supplies W8's bounded source hold (`f9cb7245`,
present in integration `9993db69`). No production changes to `source.rs` or
lane kernels are part of the fix.

A default unity envelope at `PreparedVoiceChain::begin` hid the source's held
frames. The real envelope and processor states then advanced over held silence.
The fix uses a finite unity timing envelope only while the cursor is waiting,
preserves its exact held prefix, and processes only the active suffix. Held
frames do not consume envelope, FX, send, or choke-fade clocks. Waiting voices
use the scalar path through the shared batch admission check; recovered voices
return to the existing lane kernels. Resident rendering keeps constant unity.

The finite timing envelope is bounded by both real envelope and choke-tail
budgets. Callback length includes held frames, while fade accounting uses only
active frames. This also covers hold expiry within a callback. Absolute control
and modulation clocks remain outside this fix's scope.

## Executed validation, 2026-10-08

Tests used synthetic PCM and the normal runtime, with callback heap guards.
Both comparison sides were admitted cold before pages were published, preserving
the same recovery fade. One side waited 128 silent frames. Recovery compared
256 output frames exactly after changing the native Gainer and Stereo targets
while waiting. The matrix covers unity, a 16-frame one-shot, and attack/decay;
one and two voices on one thread, plus 64 voices on two threads with an assertion
that parallel processing ran. Separate tests cover partial hold expiry and a
16-frame choke budget crossing the hold boundary.

- Pre-fix source with the corrected fixture: **0/3 passed**, semantic failures
  (recovery PCM differs; finite/choked voices retire prematurely).
- Fixed source with the same fixture: **3/3 passed**.
- Existing `control_dsp`, `dsp`, `gainer`, `paged_render`, `signal_trace`, and
  `svf` integration tests: **42/42 passed**.
- `cargo test -p sampler-core --no-run`: **passed**.

All cargo calls ran through `~/.cache/kontakto-heavy`, one W6 job at a time.
Logs: `~/.cache/kontakto-w6/cold-hold/{red,green,existing,no-run}.log`.
The red run temporarily reversed only this fix's production diff; the patch was
restored before the green run. W9 independently reported a semantic scalar-chain
failure in its separate cold-start fixture before receiving this fix.

W9's lane helper must call `dsp::levels(v, len, 0)` and retain the shared waiting
exclusion. W9 owns its separate paged-render fixture. No whole-corpus, native
host fidelity, or callback CPU performance claim follows from these tests.
