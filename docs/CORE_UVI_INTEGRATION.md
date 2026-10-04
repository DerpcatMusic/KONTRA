# Core/UVI integration checkpoint — 2026-10-05

This integration starts from recovery commit `834c56f` and incoming UVI commit
`4bffbb1`. Work stays in the isolated integration checkout. It does not replace
installed plugins, publish a release, or establish full Kontakt/Falcon parity.

## Ownership and execution

- A rack slot owns `RackPart`: either an ordinary `Engine` or a retained
  `ProgramBank`. Instrument-only editing returns an optional engine; a bank
  never exposes its selected program as an editable instrument.
- Workers prepare complete replacements, including sample pools and FX. Audio
  swaps prepared owners and returns replaced owners for worker-side disposal.
- Canonical exact-note roots survive bank/instrument replacement. Rejected
  NOTE_END delivery continues to block identity reuse until acknowledgement.
- MIDI1 Program Change is connected to saved bank port/channel selection in
  both typed host and exact raw event paths. Ordinary instruments still report
  unsupported selection. MIDI2 Program Change/bank-select is not admitted.
- Bank render phases participate in the rack's chronological boundaries. Inner
  amplifiers share the container's work-based freshness; dormant siblings retain
  their scalars and a never-fed late sibling ramps from zero.
- The container supplies the voice-budget policy, but each original inner
  program counts and selects its own victims. There is no global victim pool.
- UVI and NativeUI use one Lua 5.1 configuration. NativeUI supplies table helpers;
  editor metadata is Send-safe while its actual Lua VM remains thread-local.

## Explicit limits

Bank admission still rejects scripts, articulation/MPE, shared native AHDSR
ranges, direct inner output destinations, standard inner CC7/10 controls and
unsupported alignment/override state. Nonunit inner gain/pan with returned-send
FX is rejected: native mode2 requires an additional retained return amplifier,
which this checkpoint does not implement. Nonunit plain-insert programs are
admitted and checked separately. No native host-binding equivalence is claimed.

Native freshness and main-return evidence is retained in the original checkout's
`artifacts/core-native/program-container-saved-metadata-contract.md` and
`capture_program_bank_fresh.py`. The sparse-bank oracle now implements that
shared receipt independently rather than assuming standalone-engine freshness.
Its former `0.16012746` PCM mismatch is retained in
`/tmp/kontra-pi-bank-playback.log`; the updated scenario keeps its PCM tolerance,
exact release/terminal assertions and zero-allocation/free check.

## Bug found by the integration checks

The rack-growth fixture reproduced a cross-partition PCM difference at frame430
following the exact envelope terminal at429. Voice-local pre-amplifier sends
were still fed until the longer render interval ended. Voice rendering now
stops local source/insert/send work at the captured exclusive source, envelope
or fade terminal; program-owned return effects keep their tails.

The growth scheduling oracle uses the deterministic offline instruction budget:
one-frame debug callbacks previously had only about8 microseconds of wall-clock
allowance and could defer KSP work. Its complete fixed/irregular PCM comparison
is unchanged. The final FIR silence assertion allows a peak below1e-8 rather
than requiring bit-zero FFT roundoff. The separate live rack scenario is
unchanged and reports a drained FIR peak of4.1909518e-10.

## Focused checks at this checkpoint

All commands use the isolated target
`/home/derpcat/.cache/kontakto-uvi-checkpoint-target`.

| Check | Result |
| --- | --- |
| Library, no default features | Passed after formatting and terminal fix |
| All root targets, `uvi,standalone` | Passed after formatting and terminal fix |
| Program-bank unit scenarios |6 passed: sparse PCM/roots/releases/decoder, unwind restoration, nonunit startup, return-FX admission, max1 per-program policy, replacement/backpressure |
| Live20→5 rack chronology with pedals and routed FX |1 passed, fixed and irregular PCM, no heap operations |
| Rack growth at global333 with listener/wait/generated child |1 passed after terminal fix, exact429/4477 ends, fixed/irregular PCM, no heap operations |
| Ordinary-instrument Program Change diagnostics |1 passed |
| NativeUI hidden-image Lua control/layout |1 passed |
| UVI restore/adopt/delay/keyboard/gain/pan/UI |1 passed |

The deliberate panic in the canonical-root test is caught to test unwind
restoration; it is not a failed test. Compilation still reports existing
warnings. These are small synthetic functional checks, not full library,
listening, real DAW, cross-platform provider-runtime or shipping-profile tests.

## UI follow-on

The user also authorized integration of PR33 head `b94ad84`, based on main
`ac2adc9`. Its recorded CI results apply to that candidate, not this newer core.
Import its shared native bridges and preserved X11/browser behavior as a
separate checkpoint. Preserve the core's MIDI ownership and retained-image
resource changes rather than replacing the sampler with the audited snapshot.
