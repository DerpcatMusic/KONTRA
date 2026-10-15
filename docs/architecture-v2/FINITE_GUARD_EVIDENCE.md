# Prepared DSP state validation, 2026-10-14

Cycle samples in the 300-voice EQ workload identified retained-state guards and
inactive color histories as measurable work. Previously every processor's
state check visited the entire color-state superset, including decimator,
saturator, damping, tone, widening and Redux histories, even for an EQ stage.

Validation now follows immutable prepared processor ownership. All common
double-precision state remains checked. Color units check every subunit they
own, including a bypassed or not-yet-started subunit. Native KONTAKTFX filters
retain the conservative full color guard because their history uses that slot.
Other processor kinds cannot write or subsequently enable color subunits.
Voice-slot reuse resets all histories before another chain claims the slot.
Output finite checks, dropped-block behavior and fault resets are preserved;
no DSP kernel, effect, quality setting or CPU-meter rule changes.

Both voice and bus paths use the same stage/state mapping. A length mismatch
fails validation rather than truncating the check. Exhaustive processor-kind
matching forces a new prepared kind to choose its validation policy.

## Isolated validation measurement

Reproduce with an optimized test build and the ignored
`dsp::tests::finite_guard_stage_evidence` test. It compares the frozen original
guard and the scoped guard in the **same executable**, using the same 300 sets
of four state cells and a prepared three-band native EQ plus Stereo chain.
Common histories contain finite nonzero values; a nonfinite active history
must fail both guards before timing begins. This measures validation work,
not signal processing or the whole plugin callback.

CPU: Ryzen 7 7800X3D, caller pinned to CPU 5; `corpus` optimized profile,
unmodified shared target configuration. No rustc process was present at the
start. The rest of the host remained running. Four alternating-order pairs,
600,000 voice-chain checks / 2,400,000 processor-state checks per sample:

| Pair | Original microseconds | Scoped microseconds |
| --- | ---: | ---: |
| 1, original first | 127,788 | 33,809 |
| 2, scoped first | 123,880 | 28,492 |
| 3, original first | 127,423 | 28,852 |
| 4, scoped first | 124,871 | 28,747 |

Median guard time is 126,147 → 28,799.5 us, approximately **77.2% less for this
validation-only workload**. The first scoped sample is retained. This does
not establish a full-callback improvement or meet the requested 300-voice
1% KONTRA-meter target. A whole-runtime witness also containing wider EQ
dispatch must be labeled as a combined change.

Raw receipt:
`/mnt/Windows11/DEV_WORKSPACE/PluginArtifacts/kontra-diagnostic-fixes/work-t3-726313e8/finite-guards-optimized-receipt.log`.

## Correctness evidence

448 core library tests passed, with four existing explicit ignores (before
adding the ignored performance receipt). All 15 bus and eight DSP integration
tests passed. These cover voice/bus fault isolation, source completion,
retained tails, generation replacement, slot reuse, multivoice output and
audio-thread heap guards. Added probes poison owned gain, hold, noise,
stereo, damping and biquad histories before first use: every active guard
rejects them. Existing color law fixtures compare scoped and full guards on
reachable histories after processing. Common NaN/infinite histories and
state-map length mismatches remain rejected.
