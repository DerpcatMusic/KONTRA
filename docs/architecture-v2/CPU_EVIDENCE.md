# CPU and stream deadline evidence, 2026-10-14

The requested target is **300 sounding physical voices at 1% on KONTRA's meter**.
It is not achieved by this work. A 300-voice integer-rate, effect-free synthetic
core case reaches a 0.90% median at 256 frames, but it neither exercises a real
authored instrument nor measures the entire plugin callback. Pitched, filtered,
modulated and EQ workloads are substantially more expensive. Do not present
the integer-rate result, a low-voice library run, or a core-only timing as proof
of the requested target.

The later actual `Sampler::process` witness with 300 distinct owned PCM sources
measures 1.886% integer-rate, 18.205% fractional and 39.007% 3EQ+Stereo mean
busy/span at 48 kHz/256 frames. It reports zero faults and deadline misses in
that unpaced interval, with large tails retained. The exported DAW adapter,
authored scripts, disk decoding and UI painter remain outside that witness.
See [whole callback evidence](WHOLE_CALLBACK_CPU_EVIDENCE.md).

## Metric and reproduction conditions

KONTRA's meter accumulates entire `Plugin::process` elapsed wall nanoseconds in
`busy_ns`, and callback audio duration in `span_ns`. The UI samples the delta
ratio every 100 ms, eases it by averaging with the previous displayed value,
zeros values below 0.005, then rounds the percent. The meter includes host-event
handling, core work, mixing and output work. None of those rules was changed.

The core witnesses report these separately:

- callback wall time / audio deadline: the closest component of the KONTRA
  metric; wall samples above 100% are actual callback deadline misses;
- caller thread CPU time / deadline: excludes time the caller is descheduled;
- process CPU time / deadline: includes render workers, and, in the paced real
  harness, decoder workers. More than 100% can represent concurrent cores rather
  than a wall deadline miss. The `deadline_misses` field inside a CPU-clock
  summary means CPU budget exceedances; only the wall field identifies deadlines;
- paced whole-phase process CPU: also includes the harness's hashing, probing,
  event and decoder overhead. It is explicitly not a plugin or OS-system meter.

Hardware: AMD Ryzen 7 7800X3D, eight cores / 16 logical CPUs, boost enabled,
x86-64 Linux `7.3.0-rc6-2-cachyos-rc`. Compiler: rustc 1.99.0
`b940084d7`, LLVM 23.1.1. Audio: stereo, 48,000 Hz, 64 and 256 frames
(1,333.33 and 5,333.33 us deadlines). Builds use `--profile corpus`: release
optimization, ThinLTO, unwind panics, retained symbols; no custom target CPU,
fast-math, reassociation, quality reduction or disabled authored effects.
Runtime SIMD dispatch selects the supported hardware implementation. The
shared cargo target configuration is used without overrides.

All raw receipts and frozen executables are under
`/mnt/Windows11/DEV_WORKSPACE/PluginArtifacts/kontra-cpu-20261014/`.
Binary and library payloads are not committed. The machine also runs a VM,
Bitwig and filesystem/background jobs; they were not stopped. Timing windows
exclude builds/probes from this team when stated, but are not an isolated host.
Keep the recorded long tails and adverse paired runs. Some paired tests pin
the caller to CPU 5; most matrix tests are unpinned.

## Sounding synthetic reference and conservative DSP improvement

`sampler-core/examples/voice_cpu.rs` starts independent physical note owners,
verifies their active count and nonzero PCM energy, and reports a PCM hash,
nonfinite frames, voice drops and stream underruns. Its PCM contains stereo
sine plus noise, and each note has a different gain. Above 256 voices the gain
spacing remains distinct instead of clamping velocities to unity. It warms
512 blocks. Resident and paged cells use the same real PCM; paged cells preload
four actual pages and retain the worker endpoint. Cache admission is sized for
the per-voice reservation contract, even though pages are shared. These paged
cells measure page access overhead, not asynchronous disk/decode service.

Plain means exact integer-rate playback. Fractional means 3.17-semitone upward
transposition through the existing realtime sinc bank. Filtered adds a 3 kHz,
Q=0.7 SVF. Modulated adds native envelope/LFO pitch, pan, cutoff and attenuation
work. EQ uses three KontaktLegacy bands (gains +6, -4, +3 dB) and non-pseudo
Stereo width/pan. They are not equivalent workloads.

The before executable predates `dafce4c7c`; the after executable includes it.
That commit interleaves independent left/right EQ recurrences while retaining
each channel's floating-point operation association. Stereo reuses constant
targets only after the original recurrence reaches an exact fixed point,
including signed-zero checks; moving controls and pseudo delay clocks retain
their original behavior. No effect is bypassed.

100 voices, 256 frames, resident, one thread; median core wall/deadline percent:

| Workload | Before | After |
| --- | ---: | ---: |
| plain | 0.30 | 0.31 |
| fractional | 5.33 | 5.37 |
| filtered | 9.22 | 9.10 |
| modulated | 13.41 | 13.44 |
| three EQ plus Stereo | 22.20 | 17.18 |

The 64-frame EQ median changes 22.28% to 17.18%; paged 256-frame EQ changes
22.52% to 17.96%. All 62 before/after matrix PCM hashes match bit for bit.
Raw: `synthetic-before.json`, `synthetic-after.json`. The first plain/fractional
after cells overlapped a small asset-agent test finishing and are marked as
contended, rather than clean acceptance. EQ cells had no team build active.

Adverse repeats are retained: unpinned caller-CPU EQ medians were before
1,189.2/1,185.4/1,175.6 us and after 1,294.9/933.2/919.1 us. The first candidate
is approximately 9% worse. CPU-5 alternating repeats give before
1,172.2/1,167.2/1,339.9/1,493.8 us and after 905.7/910.0/915.1/920.5 us.
All hashes still match. Raw: `eq-stereo-paired.json` and
`eq-stereo-paired-affinity.json`. Approximately 22% at comparable typical
clocks is a defensible stage-workload improvement, not a universal latency bound.

Idle medians are 0.10 us at 64 frames and 0.20 us at 256 frames. CPU probe
overhead is visible (approximately 0.42 us caller CPU); it is not silently
subtracted. The idle capacity is 100, with zero active voices.

## 300-voice matrix

Frozen `voice_cpu-300-before-seam` includes `dafce4c7c` and the distinct-velocity
change, built from the `b45321fa0` ancestry before the virtual seam experiment.
`synthetic-300-before-seam.json` has 25 cells, 1,500 measured blocks per cell,
512 warmup blocks, 300 verified physical voices, no nonfinite frames, voice
drops or stream underruns. No team build/probe was active in this matrix window.
Wall misses below are retained; page preloading makes them compute/scheduling
deadline faults rather than decoder starvation.

256-frame results (p99.9/max in us; total means all-worker mean core percent):

| Residency | Workload | Threads | Wall p50 % | Wall p99 % | p99.9 us | Max us | Wall misses | Total % |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| resident | plain | 1 | 0.90 | 1.34 | 124.1 | 137.7 | 0 | 0.95 |
| resident | plain | 4 | 3.57 | 4.74 | 316.2 | 664.3 | 0 | 9.77 |
| resident | fractional | 1 | 16.24 | 47.67 | 2901.8 | 2953.6 | 0 | 17.74 |
| resident | fractional | 4 | 7.51 | 15.26 | 1057.5 | 1071.8 | 0 | 25.62 |
| resident | filtered | 1 | 28.45 | 57.98 | 5234.0 | 5513.2 | 1 | 30.36 |
| resident | filtered | 4 | 9.89 | 18.49 | 1123.5 | 1126.9 | 0 | 33.75 |
| resident | modulated | 1 | 39.99 | 69.62 | 4208.1 | 5465.7 | 1 | 41.38 |
| resident | modulated | 4 | 17.65 | 25.52 | 1572.1 | 1635.1 | 0 | 46.93 |
| resident | EQ/Stereo | 1 | 54.71 | 84.95 | 4998.3 | 6279.4 | 1 | 56.57 |
| resident | EQ/Stereo | 4 | 16.84 | 26.57 | 1618.5 | 1941.6 | 0 | 60.76 |
| pages | plain | 1 | 1.15 | 2.13 | 140.4 | 153.2 | 0 | 1.21 |
| pages | fractional | 1 | 16.49 | 67.58 | 4041.3 | 4359.1 | 0 | 17.96 |
| pages | filtered | 1 | 29.06 | 80.54 | 6086.7 | 7523.2 | 2 | 31.00 |
| pages | modulated | 1 | 42.86 | 94.81 | 6236.0 | 8111.8 | 7 | 45.31 |
| pages | EQ/Stereo | 1 | 56.10 | 108.68 | 6218.5 | 6297.4 | 26 | 58.28 |

At 64 frames the one-thread resident medians are 2.50/19.89/28.03/39.88/54.13%
for plain/fractional/filtered/modulated/EQ respectively. Smaller deadlines
increase fixed overhead and scheduler vulnerability. Four threads help the
heavy 300-voice workloads but substantially hurt the cheap plain workload and
consume more total core time. There is no evidence supporting a blanket
increase of worker count, voice cap or cache size.

## Stage attribution and counters

Constant native EQ now has a conservative cross-voice lane path (`5ea5723b3`).
Eligibility requires constant prepared KontaktLegacy controls and enabled state;
generic/live/script EQ remains excluded. Per-voice coefficients and histories
are independent. An inherited state that has not reached its exact constant
targets keeps the original scalar native cadence. Unequal ends, zero-length
voices and processor tails rest their own histories. The exact oracle covers
1/3/8 voices, changing block lengths, distinct coefficients, inherited cadence,
zeros/denormals, and 192 real source voices against identical separate chain
owners at 1/2/4 threads with zero heap calls.

Pinned 300-voice EQ pairs, 256 frames, 600 measured blocks each, retain identical
PCM hashes across four alternating rounds. Typical caller CPU medians change
2,845.6/2,852.2/2,902.2 us to 2,380.8–2,399.8 us (about 16–18% lower).
The first before run is an adverse 4,182.7 us caller / 150.35% wall outlier and
is not discarded. Raw: `eq-lanes-300-paired.json`.

`synthetic-300-final.json` repeats all 25 original cells with exact matching
hashes and physical counts. It does **not** establish safe tail deadlines:
resident 256-frame EQ hits p50/p99 73.76/279.21% at one thread (280 misses),
58.85/471.34% at four threads (470 misses). Caller/process CPU also rises,
so this cannot be explained solely by caller descheduling. Those bad runs
remain the final matrix; no selective replacement with cleaner cells is made.
The 64-frame EQ median is 44.76% resident / 45.81% paged, versus 54.13/55.31%
before; the paged 256-frame EQ median is 46.25% with zero misses in that cell.
OS/background variation and the lack of an isolated host limit acceptance.

Leaf cycle sampling is separate from unprofiled timing. The earlier 100-voice
before profile has 79,286 samples, zero lost: EQ stage 44.32%, sinc source run
21.99%, Stereo 7.52%, chain render 3.15%, voice render 2.78%, state-finite guard
2.34%, chain finish 2.16%, plus roughly 6% in other inactive color-history
finite scans. It overlapped an optimized build, so its elapsed timings are not
acceptance. Raw: `eq-before.perf.data`, `eq-before.perf.txt`.

The 300-voice optimized after-DSP leaf profile has 21,200 samples, zero lost,
no team build active: native EQ 38.67%, sinc run 28.15%, prepared chain render
4.00%, state-finite guard 3.39%, voice render 3.32%, decimator-history finite
scan 2.27%, chain finish 2.18%, Stereo 1.99%, additional state scans 1.68%.
These are sampled cycle shares, not additive direct stage timers. They point
to EQ, source kernels and generic state validation rather than parameter reads
alone. Raw: `300-eq-before-seam.perf.data` and `.perf.txt`.

The same workload's independent `perf stat` records 50,249,500,676 cycles,
151,511,053,004 instructions (IPC 3.015), 14,629,677,717 branches,
6,757,306 branch misses (0.0462%), 3,367,260,883 generic cache references and
255,008,903 generic cache misses (7.57%). Events were multiplexed at about
66.6%; scope includes startup, warmup and measurement. AMD generic event
counts are not Intel top-down categories or proof of DRAM bandwidth saturation.
Raw: `300-eq-before-seam.stat`, `300-eq-before-seam-counter.json`.

Permitted transport is launching one's own child:
`perf record -F 1999 -e cycles:u -- <frozen witness> ...`.
No host permissions or sysctl were changed. Attach/cpu-clock stack attempts in
the UI investigation were denied; do not imply those unavailable stacks were
collected. User-only call chains need explicit user-callchain filtering on
this host; no raw DWARF memory-stack dump is published.

## Real streaming, policy differences and reader churn

The real Afflatus Chapter II Brass `4 Horns KS.nki` sustain harness plays eight
distinct keys through Kontakt loading, MIDI ingress and scripts and verifies
128 physical voices throughout held playback. Before DSP, one-thread held
wall p50/p99 is 29.88/36.83%; two threads gives 21.72/26.01% while process core
use rises. These are 128-voice core measurements, not 300-voice meter proof.

The original harness failed to set runtime stream horizon and did not bootstrap
the render/service order like the frontend. Its receipt with 142 cumulative
underruns is retained as a deficient harness result. Correcting these produces
261 underruns, all in the attack; held/release/post-release phase deltas are
zero. Attack max wall is 3.14 ms, below the 5.33 ms deadline. This distinguishes
stream starvation from missed compute deadlines, but is not proof of a
production frontend fault: that harness uses max-step 4, eager unlimited heads,
2,048 capacity and 256 frames. Production V2 uses max-step 16, lazy Auto heads
with an 8 MiB budget, 1,024 maximum voices and 128-frame render fragments.
Measured head latency and resulting head size vary between loads. The after-DSP
diagnostic has an 828-frame head, zero underruns and held p50/p99 25.84/30.27%.
All comparable held/release PCM hashes match (`c8fc34eff59b8554` and
`45250bdc83ec967b`). Onset hashes with starvation are not expected to match.

Raw receipts: `afflatus-256-8-1-before.json`, `afflatus-256-8-2-before.json`,
`afflatus-256-8-1-warm-before.json`, `afflatus-256-8-1-corrected-before.json`,
`afflatus-256-8-1-dsp-after-diag.json` and corresponding `.log` files.

Opt-in `KONTRA_STREAM_AUDIT` counters operate only on decoder workers and print
aggregate numbers, without source names, scripts, PCM or resource payloads.
The original 16-reader worker cache reopens 3,767 of 3,828 page jobs (98.4%),
with 3,703 evictions. Individual jobs reach 23.6 ms, longer than the 17.25 ms
authored head. The first 128 jobs per worker consume 71/52/111/79 ms. Decode
burst latency, reopening churn and shared storage need measuring separately
from core rendering.

The new cache limits known WAV/NCW retained owned userspace capacity to 8 MiB
across workers and 128 entries per worker. It charges vector storage, reader
boxes, paths, byte buffers, all NCW channel/offset-table capacities, scratch
growth and new File Arc headers. Existing shared handles/keys are not deep
charged again. Allocator/kernel FD overhead and transient current decode
workspaces are separate from that retained-capacity limit. Oversized readers
serve their current worker job and are then released; tiny budgets retain
neither readers nor vector capacity. Unknown custom closure heaps retain the
legacy 16-entry cap and are explicitly outside the expanded known-capacity
claim. LRU lookup remains bounded and all operations remain off the audio thread.

Focused tests preserve NCW PCM, reuse 40 small readers without the 16-entry
churn, evict grown/oversized workspaces, enforce zero/tiny budgets, and preserve
the opaque entry cap. The vendor NCW workspace test checks actual capacities
after a partial final block. The real after-cache receipt records 859 opens
for the same 3,828 page jobs (22.44%), with 531 evictions. Summed measured
open time falls from 39,893 to 15,649 us and decode time from 410,129 to
327,197 us; these aggregate diagnostic times are not clean paired acceptance
because a separate external `process_lab` build appeared during the window.
Each worker's observed peak charged retention is 2,083,506 bytes, totaling
8,334,024 bytes below 8 MiB. Held/release hashes remain exact, but 32 onset
underruns remain (zero held/release deltas), and head size differs 828 to 817
frames. No zero-dropout production improvement is claimed. Raw:
`afflatus-256-8-1-reader-after.json` and `.log`.

The actual production-policy `V2Loader`/`V2Core` example's earlier Brass
`strings` schedule peaks at only 12 voices, mean 0.78, with 192 steals and three
silent notes. This is a **rejected high-polyphony witness**, retained in
`v2-afflatus-256-before-reader.json`. Native scripts, articulation keys and
choke policies must remain enabled. A held playable chord and an exact typed
CLAP-ID schedule are added to obtain measurable real high-polyphony cases;
requested IDs do not substitute for actual physical or audible voice counts.
The exact-ID owned five-second stereo WAV receipt requests 300 owners but
sustains 128 voices, with 172 capacity drops and nonzero PCM energy; it is also
rejected (`v2-owned-300-seam-after.json`). The parent traced this to the separate
fixed 128-entry host/MPE note transport capacity, rather than the runtime's
voice maximum. The authored Dolce sustained chord is completely silent with
zero active voices and 44 missing features (`v2-dolce-held-reader-seam-after.json`);
it is a compatibility finding, not an idle-performance success.

## Validation and next hypotheses

EQ recurrence tests compare state and output bits over 12,000 moving-coefficient
frames, edge/zero/denormal inputs and block partitions. Stereo tests retain
signed-zero, moving-ramp, state and pseudo-ring behavior. DSP, multicore exact
output and zero-heap, realtime, source, paged-render and resample regressions
pass. The parent reports the core unit suite passing 441 tests before the new
seam tests. The seam prototype additionally tests exact scalar PCM/phase/
envelope/release across reverse, wrap, ping-pong, both crossfade laws, seven
rates and both quality modes, plus missing-page atomicity and ready prefixes.
It excludes wavetable, multiple loop slots and active mipmap source paths.
It must pass optimized paired timing before becoming a promoted optimization.

The first whole-stage seam receipt failed against the preexisting scalar path;
those failed receipts are kept as `seam-cpu-receipt.log` and
`seam-partition-test.log`. Investigation found an existing sound bug: the scalar
sampler inferred a contiguous reverse window from its endpoint indices alone.
With a 64-frame wrap and a 16-frame kernel radius, a seam window happens to
have a 32-frame reverse endpoint difference, but its interior actually wraps.
It read a different physical waveform. A traversal-span proof now prevents
that alias, and an independent virtual-tap oracle covers forward/reverse wrap
and reflection in both qualities. The corrected previous production run/scalar
path and gather agree bit for bit across successive blocks. The deliberate
correction of previously wrong seam sound is separate from optimization parity.

Research guides hypotheses rather than overriding sound semantics:

- [LLVM vectorizers](https://llvm.org/docs/Vectorizers.html) describe SLP of
  independent operations and cost-model limits for gathers. Independent channel
  or voice lanes are appropriate; serial recursive filter time steps are not
  independent. Ordered floating-point reductions must not be reassociated just
  to force SIMD. The existing source kernel already has polyphase coefficients,
  stereo SIMD and hardware FMA dispatch; another approximate table is not
  justified by these profiles.
- A bounded virtual seam gather can amortize repeated loop/crossfade addressing
  while keeping each interpolation's exact tap count and zero padding. Missing
  pages must fail preflight before committing any cursor/envelope/output state.
  Profile short loops and ordinary loops separately; gather cost can outweigh
  saved work on a short or already contiguous span.
- EQ is still excluded from the existing cross-voice lane-batch path. A later
  exact independent-voice EQ kernel could target the 38.67% stage share. It
  needs per-voice moving/held state, output ordering, masks and native/script
  modulation parity; broad chain eligibility is not safe without that model.
- Generic state-finite scans visit histories unrelated to an active processor.
  A prepared stage-specific validation plan may remove this overhead only after
  proving that all observable retained fault state remains checked.
- Adaptive parallel work estimates could avoid the measured cheap-voice penalty
  while reducing heavier callback tails. The current 32-voices-per-thread gate
  also makes a 100-voice/four-thread request remain serial. Change thresholds
  only with paired measured evidence, not a global assumption that more threads
  improve latency.
- [Linux perf security](https://docs.kernel.org/admin-guide/perf-security.html)
  explains collection scope and access restrictions. Keep permission failures
  and multiplexing in the methodology. Intel top-down guidance is not a measured
  AMD PMU categorization.

## Verified production-policy 300-source witness

After `95149e7d4` removes the independent 128-entry transport binding limit,
the exact typed CLAP-ID schedule through `V2Loader`/`V2Core` verifies **300
physical voices throughout the settled second** at both 64 and 256 frames.
The owned stereo WAV lasts five seconds at 48 kHz and contains low-amplitude
220/330 Hz sines; 300 exact host IDs use 12 overlapping pitched keys and
distinct velocities. This is a real PCM source through production policy,
without overriding authored polyphony/scripts, but it is not an authored NKI
or the outer plugin-process meter. PCM energy/peak are nonzero, and both runs
have zero capacity drops, steals, stream underruns, nonfinite samples and
event/render heap calls. Frozen adapter: `v2-cpu-audit-final`; raw:
`v2-owned-300-final-64.json`, `v2-owned-300-final-256.json`.

| Buffer | Settled voices | Settled p50 us | Settled p99 us | Settled p99.9 us | Settled max us | Settled p50 deadline % | Whole-run wall misses |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 64 | 300 | 274.795 | 434.058 | 479.459 | 524.390 | 20.61 | 2 |
| 256 | 300 | 1155.341 | 1323.395 | 1340.165 | 1517.219 | 21.66 | 1 |

The 64-frame misses are the initial 300-note callback (2,565.9 us wall,
2,175.6 us caller CPU), and mass key-up at frame 96,000 (5,075.4 us wall,
5,057.2 us caller CPU). The 256-frame miss is that key-up callback (6,427.6 us
wall, 6,310.0 us caller CPU). Begin/events peaks at 4,717.0/5,086.2 us;
render/finish peaks at 1,437.7/2,131.0 us. The steady source-render workload
fits its deadlines in these receipts, while bulk ownership/event handling
causes real callback faults. The witness timing includes PCM energy observation
and probes and is explicitly labeled. Four-second whole-phase process averages
(12.06/11.25%) include two seconds held and a silent/release phase; they must
not be substituted for the sustained 300-voice meter target.

Pre-fix release trace: the V2 host handler already captured exact `Held.id`,
but called FIFO `Runtime::note_off(input, velocity)`, which scanned all allocated
4,096 note slots for each of 300 IDs. The exact-ID physical key-up API retains
input-key bookkeeping, pedal policy, velocity,
generation and release callbacks. `release()` would bypass pedals and is not
a sound-preserving substitute. The API was committed as `13eb6624f` and checked
against FIFO behavior, pedals, stale generations, callbacks and heap guards.

The rebuilt exact-host owned-WAV witness still verifies 300 sustained physical
voices with positive PCM energy and zero heap calls, drops, steals, underruns or
nonfinite samples. At 64 frames, only initial admission now misses (2,504.1 us
wall, 1,879.2 us caller); mass release no longer misses. All begin/events work
is bounded by 1,042.9 us in this receipt, versus the previous 4,717.0 us peak.
At 256 frames there are zero misses and begin/events is at most 1,069.1 us,
versus 5,086.2 us before. Held p50/p99/p99.9 is 270.3/337.6/366.9 us at 64
and 1,029.0/1,239.0/1,248.2 us at 256, or 20.27%/19.29% of the deadline.
This remains a core adapter measurement, not the entire plugin meter.
Raw: `v2-owned-300-exact-release-64.json`,
`v2-owned-300-exact-release-256.json`.

A following event-detail receipt captures successful as well as late callbacks.
At frame 96,000, 300 release events cost 157.1/188.7 us at 64/256 frames;
the complete callbacks cost 516.5/1,750.8 us. An external synth compilation
overlapped this following window, so these exact event values are labeled
contended and do not establish clean acceptance timing. The original quiet
receipt above independently bounds all release work and proves the old release
miss is absent. Raw: `v2-owned-300-exact-detail-64.json`,
`v2-owned-300-exact-detail-256.json`. Duplicate-input admission still scans
allocated note slots and is being measured separately. Scheduled-command cleanup
and per-prepared-stage callback trimming remain separate costs.

The actual Brass saved voice policy permits 512 program voices, prefers released
voices and has a 10 ms fade; it does not explain the rejected 12-voice witness
as a 12-voice authored cap. Group metadata contains four mic positions and four
sustain dynamic layers. Metadata multiplicity still does not prove which layers
sound. The policy receipt is
`/mnt/Windows11/DEV_WORKSPACE/PluginArtifacts/kontra-diagnostic-fixes/brass-saved-voice-policy.json`.

## Wider EQ dispatch experiment: retain rejected evidence

Assembly shows why a broad SIMD claim is premature: the recursive stage
dispatcher remains an out-of-line baseline function when called by AVX2 render
closures. Forcing inline on the native EQ methods does not carry target features
through that boundary. A local dispatch around the nonrecursive recurrence emits
YMM multiply/add/subtract instructions, but large array-building helpers still
appear as baseline calls inside the loop. An isolated same-binary recurrence
receipt regresses from 95.6–98.7 ms to 127.3–128.4 ms per 100,000 calls (30–34%).
It is rejected as a speed improvement; YMM opcodes alone are insufficient proof.
Raw: `eq-local-dispatch-stage-clean.log`, `eq-inline-dispatch-calls.asm`,
`eq-local-dispatch.asm`.

Combined local-dispatch plus scoped-validation runtime pairs have typical caller
medians 2,372–2,392 us before and 2,180–2,193 us after, but one candidate is
2,399.9 us. Hashes remain exact. This is a **combined** receipt and does not
attribute improvement to width; the parent has a separate same-binary guard
witness. Raw: `eq-local-guard-combined-paired.json`. The slower monolithic kernel
was removed from the runtime path.

The accepted replacement completes four independent double-precision lane
recurrences before moving to the next four lanes. This limits live registers
without reordering any lane's temporal recurrence or floating-point expression.
Its local dispatched AVX2 assembly has YMM multiply/add/subtract instructions and
no helper calls in the temporal loop; it introduces no fused operations.
The primitive oracle covers empty/short/full blocks, inactive masks, signed
zero, denormals and final histories. The native EQ oracle also checks inherited
moving coefficient cadence and unequal tails. The 192-voice scalar-chain versus
batch comparison remains bit-exact at 1/2/4 threads with no audio allocation.

An isolated same-binary 100,000-call stage receipt improves 97.2–99.3 ms to
88.4–93.8 ms (4–10%). The whole-runtime pair then freezes the **same** coherent
source-accessor and scoped-validation code on both sides, changing only EQ.
Four alternating CPU5-pinned 300-voice, 256-frame resident pairs have caller
medians 2,126–2,141 us before versus 1,992–2,013 us after (about 6–7% lower).
All eight hashes are `fd8fcc10718a4a71`, with zero wall deadline misses and no
`rustc` observed by the half-second process monitor. Additional short checks
give 537.7→507.1 us at 64 frames and 2,216.4→2,061.5 us with preloaded pages at
256 frames, also exact with zero misses. These measure core render, not the
entire plugin meter. Raw: `eq-blocked-stage-receipt.log`,
`eq-blocked-stage-v3.asm`, `eq-blocked-300-paired-clean.json`,
`eq-blocked-short-buffer-paged.json`, `eq-blocked-native-test.log`,
`eq-blocked-chain-test.log`.

The initial whole-runtime pair set overlapped an already-running agent test
build and an external synth build after its start snapshot. It is retained as
`eq-blocked-300-paired-contended.json`; its 0–244 misses per cell and variable
caller time are excluded from quiet timing proof. This is why the clean repeat
records compilation activity throughout its window.

## Four-output sinc: the wrapper matters

The existing sinc already uses packed stereo accumulation, 64 phase rows, eight
stretches per octave and hardware-selected FMA. The new path interleaves four
independent output recurrences, preserving every output's tap-chunk order,
coefficient interpolation, stereo reduction, cursor phase adds/truncations,
envelope advancement and mix association. It uses the existing contiguous-span
preflight; incomplete groups use the established scalar frame loop. Only the
measured AVX2/FMA bank path changes. Baseline targets retain the original run;
High quality, cubic interpolation, seam fallback and octave-level policy stay
as before. No tap count, phase density or effect is reduced.

The first four-output layout loses 6–13% at the isolated stage. Assembly reveals
an out-of-line row-array helper and redundant range machinery. Explicit chunk
row initialization removes that helper and improves a clean, same-binary,
CPU5-pinned one-million-call stage from 39.4–40.3 ms to 30.7–31.2 ms (21–23%).
That is **not sufficient**: the first source wrapper still loses roughly 8–10%
in full fractional render, because its separate four-window array builder also
stays out of line. That wrapper is rejected. Explicit inline window construction
removes the second helper and is the accepted implementation. This follows the
compiler's documented [interleaving/register-pressure tradeoff](https://clang.llvm.org/docs/LanguageExtensions.html#extensions-for-loop-hint-optimizations)
and avoids [floating-point reduction reassociation](https://llvm.org/docs/Vectorizers.html#reductions).

The full matrix uses two alternating pairs per cell, 300 sounding physical
voices, 48 kHz/256 frames, one caller pinned to CPU5, 512 warmup and 400 measured
blocks. All 24 output hashes and energy sums match exactly, with zero compiler
observations throughout. Distinct assets are 300 separately allocated PCM
bodies containing different audio; distinct cursors use 300 different pitch
steps on one asset. Native envelopes, three EQs and stereo processing remain
independent per voice.

| Workload | Source layout | Before caller p50 us, two rounds | After caller p50 us, two rounds |
| --- | --- | --- | --- |
| Fractional | Shared | 823.6 / 814.3 | 790.9 / 789.0 |
| Three EQ + stereo | Shared | 1,984.6 / 1,970.2 | 1,896.8 / 1,899.2 |
| Fractional | 300 distinct assets | 849.6 / 852.1 | **1,280.6** / 801.5 |
| Three EQ + stereo | 300 distinct assets | 2,010.7 / 2,017.1 | 1,923.2 / 1,954.0 |
| Fractional | 300 distinct cursors | 855.1 / 851.6 | 797.2 / 796.9 |
| Three EQ + stereo | 300 distinct cursors | 2,059.2 / 2,067.3 | 1,988.3 / 1,984.9 |

Typical whole-render savings are 3–7%, not the primitive's 21–23%. The bold
distinct-asset candidate is a retained 51% CPU regression, with an unresolved
cause; wall and caller CPU both increase, so simple descheduling alone does not
explain it. A separate compiler-free, alternating distinct-asset repeat gives
855.2/862.8→796.1/797.3 us, exact audio, and one candidate wall deadline miss.
The full matrix has three before misses and one after miss. These receipts
support a typical gain and do not establish stable deadline tails or a 1% meter.

The primitive checks every bank and phase row, fused/unfused arithmetic, phase
edges, signed zero, denormals and finite extreme inputs. Cursor tests compare
PCM, phase/position, last sample and subsequent envelope values for every length
0–65. Source, paging, realtime heap guards, resampling, multicore, EQ-lane and
source-revision integration suites also pass. Raw artifacts:
`resample-four-stage-receipt-clean.log` (rejected first layout),
`resample-four-refined-stage-clean.log`,
`resample-four-refined-stage-clean-monitor.json`,
`resample-four-runtime-rejected-wrapper.json`,
`resample-four-inline-runtime-paired.json`,
`resample-four-inline-distinct-repeat.json`,
`resample-four-final-bits.log`, `resample-four-final-integrations.log`.

Remaining acceptance gap: a verified 300-source real instrument or owned real
PCM fixture through production policy has now been obtained for owned WAV
sources; an authored 300-source instrument and complete plugin-process meter timing,
zero starvation/nonfinite/drops/steals attributable to capacity, and stable
deadline tails under host integration. A 1% whole-callback target implies only
13.33 us at 64 frames or 53.33 us at 256 frames, shared by all 300 source/DSP
voices and frontend work. The measured pitched/effected workloads exceed that
budget by large factors; no universal promise follows from the completed gains.

## Measurement coverage and limits

The next bounded experiment reuses only resident, pre-envelope source blocks
within an existing eight-voice batch. Each hit requires an exhaustive bitwise
cursor key, source slot, chain, requested length, envelope completion/remaining
frames and tail count, plus current resident and mipmap pointers/lengths.
Retained first-read guards prevent pointer ABA during control-side publication.
The same immutable interpolation kernel belongs to the batch. Gain, envelope,
processor state, bus routing and slot-order summation still run independently.
Wavetables, native loop slots, stretching, cold holds and starvation/fade-in
states remain on the existing path; existing script/modulation batch exclusions
remain in force. No allocation, Arc clone, locking, I/O or audio-side storage
reclamation is introduced. The compact eight-entry cache occupies 11,792 bytes
in its test build; an initial full-cursor copy exceeded the intended 16 KiB
bound and was replaced before performance evaluation.

The initial cache has three passing direct state/publication oracles and 76
passing integration checks, including heap guards, exact scalar-owner versus
1/2/4-thread batched PCM, tails, buses, streaming pages and source revisions.
`source-reuse-runtime-paired.json` preserves all 24 exact hash/energy results.
Shared-source 300-voice caller p50 changed from 1,912.0 to 1,213.1 us for
3EQ+Stereo, and from 1,403.7 to 657.7 us for the filter workload in the first
round. Distinct assets and phases paid roughly 3–15% penalties. A compiler
was captured in 80 monitor observations, and several phase-workload runs had
large deadline tails on both sides. These receipts establish exactness and
identify a miss-path problem; they are not clean performance acceptance.
The unconditional cache is rejected. The retained revision performs a cheap
candidate preflight, lazily constructs the cache only for resident candidates,
and skips full keys/stores/lookups for unique sources or phases. Its five
direct oracles also check every `Begun` field, including tail settlement, and
zero/exhausted/partial tails; all 76 integration checks pass. A fresh baseline
and candidate were frozen against the same head-budget data layout.
`source-reuse-gated-runtime-paired.json` preserves the revised 24-cell matrix.
The first round has 46 compiler observations and its adverse tails remain
in the receipt. All 12 second-round runs are compiler-free, with exact hashes
and energy and zero wall deadline misses. Their 300-voice caller p50 results
at 48 kHz/256 frames are:

| Source layout | Workload | Before us | Retained revision us | Change |
| --- | --- | ---: | ---: | ---: |
| Shared asset/cursor | 3EQ+Stereo | 1,913.399 | 1,240.775 | -35.2% |
| Shared asset/cursor | Filter | 1,401.816 | 672.653 | -52.0% |
| 300 distinct assets | 3EQ+Stereo | 1,962.929 | 1,970.868 | +0.4% |
| 300 distinct assets | Filter | 1,412.177 | 1,415.707 | +0.2% |
| 300 distinct phases | 3EQ+Stereo | 1,989.029 | 2,009.489 | +1.0% |
| 300 distinct phases | Filter | 1,436.546 | 1,439.717 | +0.2% |

This is an accepted bounded optimization for exact eligible source matches,
with a measured small miss-path cost. It is not a universal 35–52% improvement:
distinct sources still need their original compute, and authored script or
modulation paths may be ineligible. One compiler-free alternating pair per
cell does not establish isolated-host tail guarantees. The matching shared
workloads still require roughly 12.6–23.3% core deadline time, far above 1%.
The actual complete callback receipt is separate:
[whole callback evidence](WHOLE_CALLBACK_CPU_EVIDENCE.md).

A further hypothesis is worker-baked interpolation tiles, not yet implemented.
It could remove repeated callback sinc work for a fixed immutable source,
exact cursor/pitch, quality and mipmap generation, with immediate original-path
fallback on every cache miss or control change. Each tile must also carry the
exact continuation state, preserve loop/release behavior, retain its immutable
source lease and obey a fixed byte budget with control-side reclamation. A
64-frame stereo f32 payload is 512 bytes. At 300 distinct voices and 48 kHz,
fresh payload alone turns over at 115.2 MB/s; an 8 MiB payload-only cache holds
about 72.8 ms. Keys, continuation states, handoff and f64 blocks increase that
cost. These are arithmetic capacity estimates, not measured bandwidth results.
Unique source/pitch streams still require the original interpolation compute
on a worker, and live controls can invalidate work before consumption. Moving
that compute outside the callback is not a reduction in total process CPU.
Post-envelope or nonlinear per-voice FX also remain; factoring them over a
group sum changes control/history and floating-point association. No blanket
FX factoring, lossy pitch table or asynchronous result substitution is accepted.

A separate unimplemented reuse hypothesis keeps PCM assets distinct: voices
with an identical effective fraction/step, immutable polyphase bank and output
clock repeat the same `t.mul_add(b - a, a)` coefficient interpolation. A bounded
coefficient strip could reuse those exact f32 weights while every voice still
performs its original source loads, FMA accumulator recurrence, final reduction,
envelope and effects. It would not require equal sample values or asset IDs.
At the existing maximum 104-f32 padded row, 64 outputs require 26 KiB before
keys/metadata; repeated initialization and unique-phase misses could outweigh
the saved arithmetic. A future primitive and whole-path experiment must prove
the original rounding and accumulator order, effective mipmap/quality/phase
identity, live pitch and boundary fallbacks, fixed worker storage and cheap
miss behavior. No coefficient strip has been implemented or accepted.

The opt-in whole-callback stage companion uses
`KONTRA_CPU_RECEIPT_AUDIT=1`, after warmup, to enable existing V2 stage clocks.
The default remains uninstrumented. It reports the actual zero runtime buses,
128-frame frontend render limit and prepared per-voice bytes; a 256-frame host
callback therefore makes two frontend render calls. The direct core fixture
renders 256 frames at once when no chain/modulation/bus needs segmentation, so
subtracting its plain cost from the plugin result would confound cadence and
workload. No runtime bus graph is added by `CorePart::new` in this fixture.

`whole-callback-stage-paired.json` contains two alternating same-binary rounds
for plain, fractional and EQ playback at 256 frames, with 300 distinct sources,
512 warmup and 1,000 measured callbacks. All six mode pairs have exact hash/energy,
runtime counters and heap guards pass, and the frozen binary stays unchanged.
There are zero compiler observations, but the monitor captures an unrelated
`pure_va_dispers` workload in 92 busy-process intervals. The 33 wall deadline
misses across these instrumented and uninstrumented runs remain in the receipt.
This is contended attribution, not a replacement for the earlier absolute
release witness or a clean measurement of instrumentation overhead.

| Workload | Inclusive runtime elapsed us/callback, two rounds | Unattributed callback elapsed us/callback, including probe overhead |
| --- | ---: | ---: |
| Plain | 138.940 / 131.663 | 7.815 / 7.136 |
| Fractional | 1,314.194 / 1,215.144 | 9.159 / 8.390 |
| 3EQ+Stereo | 2,567.831 / 2,346.570 | 9.547 / 8.535 |

The other disjoint timed intervals cover control polling, script/controllers,
optional stream service/empty render and persistence. Index 4 of the existing
raw array is a pending-stream **counter**, never a duration. All six elapsed
intervals lie inside the callback; their sum and the measured remainder are
reported explicitly. Runtime elapsed is inclusive of source, modulation, DSP,
sum and core telemetry. Descheduling inside it is also included. These wall
fractions cannot be substituted for exclusive CPU compute shares, and the
remainder does not identify individual output/meter/lifecycle costs.

Separate uninstrumented own-child `cycles:u` leaf profiles use the same frozen
companion, retain zero lost samples, and include setup, warmup, steady render,
release and the fixture's outside-callback observers. Plain leaves include
`render_voice` 41.62%, source mixing 16.68%, libc copying 11.27%,
`audible_voice_count` 5.53% and `prepare_voice` 4.70%. The audible count is also
called by fixture assertions outside `Sampler::process`, so its entire share
cannot be assigned to KONTRA's meter. Fractional interpolation leaves account
for 75.85% plus 6.08% in its polyphase sample helper. EQ leaves include fused
interpolation 35.86% plus 1.97%, native EQ lanes 16.98% plus 2.29% in its
adapter, color lanes 9.31%, render-run work 8.49%, finish 2.91%, envelope levels
2.27% and active finite validation 1.50%. These are sampled leaves, not inclusive
call graphs or clean hardware throughput limits. The direct-span interpolation
symbol name mentions `PagedFrames` even in this resident fixture; the name
does not establish streaming reads. Raw profiles, logs and metadata are
`whole-callback-{plain,fractional,eq-stereo}-leaf.*` and
`whole-callback-leaf.json`.

The follow-up callback-group diagnostic puts ten clock groups inside the actual
`Sampler::process`, separating adoption/polling, transport/controls/keyboard,
host events, master gain, inclusive core rendering, output/scope, UI/effects,
end-block retirement, meter publication and voice readback. It records elapsed
wall time and Linux caller-thread CPU time; workers and outside observers are
excluded from the caller clock. The inclusive core group overlaps the nested
V2 stages and must not be added to them. All new objects, branches and clock
calls are `cfg(test)` only. Disabled test mode leaves the audit absent.

`whole-callback-groups-correctness.json` verifies all twelve cases: 64/256
frames, plain/fractional/EQ and auditing off/on. Each mode pair has exact PCM
hash and energy; all retain 300 sounding owners, zero runtime faults and zero
heap calls through admission, steady render and the entire 33-callback release
and tail/NOTE_END phase. Group invocation counts prove one frontend render per
64-frame host block and two per 256-frame block. The off callback duration is
also retained separately; a mostly silent release-phase average is never used
for the sustained-voice target. These concurrent correctness runs are not
performance acceptance. Diagnostic clock calls and adjacent group bookkeeping
perturb the measurements, especially small groups; the original uninstrumented
release executable remains frozen separately.

The final same-binary group run is `whole-callback-groups-paired.json`, with
per-process logs and `whole-callback-groups-summary.json` beside it. SHA-256 is
`d906d7ab9351bd58fb7c2087e1d6c97459bd185c89d66a8b829212bdb88122b1`.
Two alternating rounds use CPU 5, corpus optimizations, 48 kHz/256 frames,
300 distinct resident PCM assets, 512 warmup and 1,000 measured callbacks.
All six mode pairs have exact PCM hash/energy, 300 sustained audible owners,
zero runtime faults, zero heap calls and complete release. The continuous
250 ms monitor records no compiler observations. Of 67 busy-process records,
64 are the measured children, two are the desktop renderer and one is Claude
Desktop; ordinary VM/DAW/desktop activity remains. This is a compiler-free
desktop run, not an isolated host or exported-plugin DAW test.

| Workload | Uninstrumented mean meter %, two rounds | Diagnostic mean meter %, two rounds | Added elapsed us/callback, paired rounds |
| --- | ---: | ---: | ---: |
| Plain | 1.893 / 1.862 | 2.022 / 2.051 | 6.850 / 10.073 |
| Fractional | 17.869 / 17.878 | 18.109 / 17.949 | 12.835 / 3.787 |
| 3EQ+Stereo | 38.261 / 38.445 | 38.790 / 38.673 | 28.210 / 12.149 |

No uninstrumented steady callback misses its 5,333.333 us deadline in these
six runs. Both diagnostic EQ runs retain one miss: maxima 7,249.477 and
7,305.678 us, versus uninstrumented maxima 3,403.284 and 3,131.429 us.
The probes increase work and can alter scheduling; these misses are retained,
not assigned to a particular scheduler or arithmetic cause. Aggregate caller
CPU excludes descheduling but does not identify the cause of each individual
tail. Uninstrumented plain p99 remains 134.672 / 150.223 us, fractional
2,222.402 / 2,171.451 us and EQ 2,985.386 / 2,957.676 us. The 1% target is
still unmet, including the cheapest distinct-source case.

The following are diagnostic means over both rounds, **caller CPU / wall
microseconds per actual sustained callback**. They include group clock and
bookkeeping costs; they are not corrected estimates of uninstrumented stage
cost. Inclusive core rendering overlaps the separately reported V2 timers.

| Actual callback group | Plain CPU / wall us | Fractional CPU / wall us | 3EQ+Stereo CPU / wall us |
| --- | ---: | ---: | ---: |
| Adoption/polling | 0.491 / 0.513 | 0.500 / 0.512 | 0.524 / 0.546 |
| Transport/controls/keyboard | 0.534 / 0.539 | 0.543 / 0.547 | 0.563 / 0.566 |
| Host events | 1.475 / 1.514 | 1.476 / 1.503 | 1.490 / 1.514 |
| Audition/master gain | 1.065 / 1.079 | 1.067 / 1.079 | 1.085 / 1.095 |
| Core rendering, inclusive | 95.793 / 96.431 | 940.632 / 949.456 | 2,021.706 / 2,053.342 |
| Output mix/scope | 1.596 / 1.798 | 1.609 / 1.808 | 1.655 / 1.860 |
| UI/effect readback | 0.481 / 0.499 | 0.490 / 0.502 | 0.507 / 0.517 |
| End-block retirement | 2.398 / 2.424 | 2.416 / 2.427 | 2.454 / 2.484 |
| Meter publication | 0.601 / 0.639 | 0.608 / 0.609 | 0.626 / 0.635 |
| Voice readback | 2.708 / 2.709 | 2.638 / 2.642 | 2.601 / 2.598 |

The final probe/unattributed remainder is 0.455 / 0.457 / 0.513 us/callback
for plain/fractional/EQ. Caller CPU can slightly exceed an individual tiny
wall interval because the two clocks are read sequentially; no fabricated
CPU-to-wall inequality is imposed. The core group accounts for the large
workload-dependent growth. The voice-readback group now proves an actual
inside-callback cost, bounded by its whole measured 2.6–2.7 us group including
probes; it does not justify assigning the earlier whole-executable 5.53% leaf
share entirely to the meter or removing voice telemetry.

On the admission callback, host-event caller CPU averages 1,159 / 1,152 /
1,267 us for plain/fractional/EQ; initial inclusive core CPU is 510 / 2,183 /
2,991 us. Those are distinct from sustained playback. Across **all 33 release
callbacks combined**, host-event caller CPU is 367 / 370 / 360 us, core CPU
564 / 1,311 / 3,080 us, and retirement CPU 111 / 119 / 117 us. These totals
include key-up and tail/NOTE_END work; dividing mostly silent release callbacks
into a low average would misrepresent the sustained-voice target. The separate
uninstrumented note-off callback takes 908–916 / 1,668–1,682 / 2,679–2,696 us.

Read-only disassembly resolves the earlier libc IP to
`__memcpy_avx512_unaligned_erms+0x13`, not `memset`. Its callers still need
stack attribution before any copying optimization is proposed. The frozen
`render_voice` assembly separately contains a 512-byte scratch `memset` even
when the unmodulated path writes directly to its target, plus large prelude
moves and generic state preparation. Static presence is not measured cost:
the 11.27% copying leaf cannot be relabeled scratch clearing. A bounded
own-child call-chain capture is the next attribution step, with fixture
observers distinguished from `Sampler::process` callers. No new production
fast path is accepted from these observations alone.

The bounded follow-up user-only call-chain capture is
`whole-callback-plain-user-callers.{data,json,txt}` with its command script and
run log. `cycles:u --call-graph dwarf,8192` initially failed at event opening
because the default call-chain request includes kernel frames. Explicit
`--user-callchains` succeeds with the same `perf_event_paranoid=2`, without
changing host permissions. The failed attempt is retained separately as
`whole-callback-plain-callers*`. The successful capture samples the unchanged
group companion with auditing disabled, CPU 5, 20,000 plain callbacks and the
same warmup/setup/observer/release caveats. It records 3,591 samples and zero
lost samples. Recording stack data adds overhead; its timings are diagnostic.

The recovered immediate caller of two libc-copy leaf locations is
`prepare_voice`: those leaves account for 4.50% and 2.97% of whole-fixture
sampled cycles in this capture. `render_voice` is 30.55% self / 50.07%
inclusive, `prepare_voice` 8.28% self and source mixing 23.62% self. These
inclusive and self shares overlap and must not be added. Several outer chains
truncate with invalid frames, including the audible-count callers; an 8 KiB
stack snapshot is not a complete callback caller census. The measured actual
voice-readback clock group remains the inside-callback evidence for telemetry.

Disassembly identifies a concrete repeated-copy candidate in the recovered
path: `Voice::set_step` assigns `self.cursor = self.cursor.with_step(...)`.
The generated plain path copies the 728-byte cursor to temporary storage and
back around its step update. This is distinct from the scratch `memset`.
An approved narrow experiment replaces that assignment with a mutable cursor
step setter, retaining the same validation, pitch arithmetic and stretch
clock behavior. The existing value-returning builder remains available.
The independent old value-returning update oracle now matches full cursor
continuation and exact PCM through changing pitch, both qualities/directions,
four loop shapes, finite/released loops, tuned native slots and wavetable steps
above the ordinary sample-rate cap. Another 101 source, resampling, realtime,
scalar/batch/parallel, modulation, wavetable, stretch, revision and bus tests
pass (one existing ignored test). Raw logs are `cursor-step-unit-coherent.log`
and `cursor-step-integrations.log`. A controlled current-layout before/after
callback pair is still pending; the sampled copy share is not an accepted
whole-callback speedup or proof of the 1% target.

The controlled same-layout executables are now frozen as
`whole-callback-cursor-step-before` / `whole-callback-cursor-step-after`, with
SHA-256 `60a949e5b266f8d7a628c9d2cf87e8ccda6d82520ec3df16e1e9cb76219d121c`
and `389b07d68980534c0cacfb180c3d709f37a9a455e9284183f7b0312f8fde311a`.
Both builds verify their compiled-source hash manifests stayed unchanged;
the two manifests differ only in the approved step-update source files.
The original cursor builder/live assignment is selected with exact checked
text replacements for the baseline, then the candidate is restored. No git
restore/reset is used. Prepared/runtime layouts and all other compiled source
hashes match. `prepare_voice` shrinks from 8,226 to 8,069 machine-code bytes;
its five memcpy call sites become two. The remaining two are the separate
modulated loop-offset path, not the live pitch step.

`cursor-step-callback-paired.json` records two alternating rounds of five
actual callback cases, each with 300 physical owners, 512 warmup and 1,000
measured blocks on CPU 5, with diagnostic clocks disabled. All ten pairs have
exact PCM hash/energy, zero runtime faults, zero heap calls and complete
33-callback release coverage. This first window is **contended**: the monitor
retains 107 unrelated compiler observations and 32 busy `kswapd0` intervals.
Its p50 results are attribution, not clean acceptance or deadline guarantees.

| Frames / source layout / workload | Before p50 us, two rounds | In-place p50 us, two rounds | Change, two rounds |
| --- | ---: | ---: | ---: |
| 64 / distinct / plain | 43.750 / 44.981 | 37.531 / 37.261 | -14.2% / -17.2% |
| 256 / distinct / plain | 100.022 / 98.082 | 86.891 / 85.501 | -13.1% / -12.8% |
| 256 / distinct / fractional | 883.007 / 900.187 | 870.056 / 874.326 | -1.5% / -2.9% |
| 256 / distinct / 3EQ+Stereo | 1,997.858 / 2,025.408 | 1,963.376 / 1,975.127 | -1.7% / -2.5% |
| 256 / shared / 3EQ+Stereo | 1,243.923 / 1,202.262 | 1,195.802 / 1,204.703 | -3.9% / +0.2% |

All long tails remain: eight wall deadline misses before and four after,
including candidate plain maxima 2,193.811 us at 64 frames and 5,963.261 us
at 256 frames in round two. Those cannot be hidden by improved medians or
assigned to a cause from this receipt alone. Candidate plain 256-frame mean
meter is 1.707 / 1.733%, fractional 18.401 / 18.247%, and distinct EQ
38.987 / 39.884%; none meets 1%. This contended window alone does not promote
a numeric performance claim. The source correction
and its exact output/state oracles are checkpointed in `aa054145c`.

The subsequent bounded `cursor-step-plain-repeat-paired.json` repeats only the
plain 64/256-frame controls with the same frozen executables, two alternating
rounds and all the same callback/PCM/heap checks. Its 250 ms monitor records
zero compiler observations; the four busy-process records are filesystem
`bees` background work. Ordinary desktop/VM/DAW activity remains. All eight
runs have zero wall deadline misses and exact paired PCM hash/energy.

| Frames | Before / after p50 us, round 0 | Before / after p50 us, round 1 | Before / after mean meter %, rounds 0 and 1 |
| --- | ---: | ---: | ---: |
| 64 | 43.801 / 37.231 | 44.041 / 37.760 | 3.342 / 2.841; 3.390 / 2.898 |
| 256 | 98.672 / 85.872 | 98.632 / 85.531 | 1.895 / 1.649; 1.913 / 1.644 |

This accepts the narrow in-place update as a measured plain whole-callback
improvement: p50 falls 14.3–15.0% at 64 frames and 13.0–13.3% at 256 frames.
The 256-frame mean meter is still 1.64–1.65%, not 1%. Pitched/EQ gains remain
limited to the earlier contended evidence; no universal percentage is claimed.
All repeat tails are retained, including the first 256-frame candidate maximum
of 272.466 us versus 218.464 us before, despite better median and p99. These
short unpaced measurements establish a compute improvement, not isolated-host
deadline reliability or an authored streamed-instrument acceptance.

The next direct-mixing SIMD wrapper is **rejected for production**. It exists
only in `source/mix_experiment.rs` behind `cfg(test)`. Assembly confirms its
forward/reverse constant kernels use YMM instructions with no FMA contraction;
the existing production loop uses SSE. An oracle matches PCM bits and full
envelope continuation for changing gains, constant/moving/curved/released/AHD
and native envelopes, short spans, unequal source/output lengths, reverse
traversal, signed-zero and subnormal operands.

The first `mix-simd-stage.json` is rejected evidence: it overlaps six compiler
observations, and disassembly reveals the test-only candidate specialized its
fixed channel gains while the general production baseline retained runtime
arguments. The corrected benchmark makes gain arguments opaque outside each
timed region and varies channel gains in the oracle. Its new assembly no
longer embeds the fixed channel gains. Corrected sources and the original
diagnostic are retained; the correction uses ordinary `git absorb` fixups,
without rebasing shared history.

`mix-simd-variable-gains-stage.json` captures four alternating comparisons per
case in the same optimized executable on CPU 5: 50,000 blocks per algorithm,
or 10,000 for native envelopes. All 48 hashes match, zero compiler observations
are recorded, and the busy-process monitor sees only the measured child.
The median candidate/original elapsed ratios are:

| Primitive case | Forward ratio | Reverse ratio |
| --- | ---: | ---: |
| 3 frames, short-span fallback | 1.269 | 1.065 |
| 128 frames, unity envelope | 0.906 | 1.040 |
| 128 frames, sustain 0.8 | 0.921 | 0.943 |
| 128 frames, sustain zero | 0.931 | 0.939 |
| 128 frames, moving envelope fallback | 0.925 | 1.004 |
| 128 frames, native envelope fallback | 1.003 | 1.005 |

The short forward span loses in every comparison (ratios 1.109–1.476).
Long-constant improvements do not pay for a generic wrapper with these edge
costs, and native-envelope changes are unresolved near parity. The moving
forward fallback improves despite retaining the original math, demonstrating
that wrapper/inlining/layout also affects this experiment. These are short
stage measurements, not complete callback percentages or an exhaustive tail
test. The failed stage gate prevents production promotion; no full-callback
gain is claimed for this wrapper. Native envelopes deliberately stay outside
the constant fast path because their destination clocks must keep draining.

| Stage or cost | Evidence | Remaining gap |
| --- | --- | --- |
| Host admission/key-up | V2 begin/events wall and caller/process CPU; exact 300 source owners, fixed mass key-up faults, [isolated admission lookup attribution](ADMISSION_CPU_EVIDENCE.md) | Authored script events and layered host-end ownership cost |
| Script/controllers | Real Afflatus MIDI/script path, error/fuel/service counters; offline UI initialization profiles in companion evidence | Separate sustained audio-script/controller stage CPU under a verified 300-source authored patch |
| Source/interpolation | Leaf cycle shares, 100/300 plain versus pitched controls, exact seam and four-output stage pairs; bit-exact shared/distinct-asset/phase whole-render matrix | Authored 300-source and entire plugin-process source-stage attribution under live pitch changes |
| Native modulation/filter | Separate sounding filtered/modulated workloads with wall/caller/worker tails | Inclusive cycle stacks or bounded direct stage clocks for individual envelope/LFO/filter components |
| EQ/color FX | Cycle shares, exact recurrence/state tests, sounding 192/300 scalar/batch/parallel comparisons, rejected wider kernel | Moving/script EQ batching and other authored effects under real 300-source load |
| Summation/lifecycle/state guards | Render/finish cycle shares, parent same-binary validation witness and exact-ID release receipt | Summation alone versus cache/ownership work; layered host-end ownership cost |
| Master gain | [Isolated parent evidence](MASTER_GAIN_EVIDENCE.md), exact smoothing/automation parity | Whole plugin-process measured gain contribution; isolated stage is only about 0.003% at 128 frames |
| Decoder/cache/storage | Job/open/eviction/capacity totals, first-128 job latency, phase underrun deltas | Cold production-policy 300-distinct-source demand, storage queue/wait and onset-deadline proof |
| Worker scheduling | 1/2/4-thread wall versus caller/total CPU, bad-tail receipts retained | Isolated host/worker wake-to-run latency and adaptive policy validated against real deadlines |
| Outer plugin/UI/host | Actual meter formula and complete 300-distinct-source Sampler::process busy/span receipt; companion offline UI/resource profiles | Exported live host with authored scripts/streaming and UI refresh; exclusive callback stage attribution |

This coverage table deliberately distinguishes completed counters from unresolved
stage attribution. Flat sampled shares, isolated stage timers, paced process CPU
and whole plugin wall time have different denominators and cannot be added into
a fabricated complete callback breakdown. [AMD's SIMD validation guidance](https://www.amd.com/en/blogs/2026/understanding-avx-512---validating-usage-on-amd-epyc-.html)
also distinguishes ISA capability, emitted instructions and executed workload;
its EPYC generation-specific events are not assigned to this Zen 4 Ryzen host.

The next structural experiment is **test-only coefficient reuse**, checkpoint
`1e9bb6d10`. Voices reading different PCM can still share the interpolation
weights when their exact fractional phase, step, immutable kernel bank,
fused/unfused arithmetic mode and output length match. The bounded primitive
holds one 64-frame tile: 26 KiB of weight payload, at most 27 KiB including its
tracked struct. Its allocation and initial page touch occur before timing;
cohort detection and fresh row construction belong inside timing. Plain,
unsupported-quality, short and unique-phase cases skip row construction.
No PCM, cursor, envelope, processor state or asset lifetime is cached.

Three optimized tests pass against the original `dot_four` arithmetic in both
fused and unfused modes. They cover signed zero, subnormal fractions/inputs,
hostile steps up to two, fresh phases, changed source contents, changed key
fields, missing/stale views and two/four/eight-source cohorts. These are raw
pre-envelope output tests, **not** full Cursor, looping, missing-page or Runtime
oracles. The frozen executable is `coefficient-reuse-primitive`, SHA-256
`156a8e3c27ec545e64decf2e9b69c2359857eae868d670799388a2c3dce9c41a`;
its command/source manifest is `coefficient-reuse-primitive-freeze.json` and
the passing log is `coefficient-reuse-final-correctness.log`. The ignored stage
diagnostic was initially unrun at this checkpoint. No production change is
claimed.

The subsequent `coefficient-reuse-stage.json` **rejects this tile design**.
The same frozen executable, pinned to CPU 5, compares four alternating orders
for each cohort, with 2,000 fresh 64-frame blocks per algorithm. All 16 output
hashes match. Its 0.32-second monitor records zero compilers and only the
measured child above the busy-process threshold; this is a short stage window,
not a host deadline test. Cohort detection, exact key checks, row construction
and consumption are timed; allocation and initial storage touch are not.

| Raw source cohort | Median candidate / original elapsed | Four paired ratios |
| --- | ---: | --- |
| Two aligned voices, no tile fill | 1.008 | 1.012, 0.968, 1.004, 1.037 |
| Four aligned voices, fresh tile | 1.751 | 1.774, 1.732, 1.653, 1.770 |
| Eight aligned voices, fresh tile | 1.293 | 1.296, 1.266, 1.289, 1.334 |
| Eight unique phases, no tile fill | 1.013 | 1.015, 1.045, 1.011, 1.007 |

Every aligned four/eight-voice comparison loses despite computing each weight
only once. The new weight stores/loads, layout and register pressure need
assembly/PMU attribution before assigning a cause; fewer arithmetic operations
alone did not produce faster execution. The failed stage gate prevents a
production integration or claimed callback improvement. A later distinct
experiment could share a small number of coefficients directly in registers
across voices, retaining each original accumulation order and avoiding a
materialized tile. That is a hypothesis, not an implemented optimization.

Any later runtime prototype must derive keys from the actual run after
expression pitch and source projection, use the selected kernel/FMA mode,
keep each source window and guard current, and preserve each cursor's phase
recurrence, envelope and DSP continuation. It must avoid spending tile setup
on lanes already served by resident source reuse. Native variable-step loops,
reverse, mipmap transitions and short/partial runs need proven fallbacks or
their own exact oracles. Preflight/miss costs, control-side scratch ownership,
parallel workers and actual 300-voice callback shared/distinct/phase/dynamic
cases are acceptance requirements. This experiment does not change the open
300-voice, 1% meter result.

## Deferred unused voice scratch initialization, 2026-10-15

The next current-source baseline is frozen as
`kontra-cpu-20261015-render/whole-callback-current-baseline`, SHA-256
`563db11c64418a017ab7cd77c8edb3f1d377cb3afea30eaaa4342ecef1bac5a7`.
The source cohort is `308bf633b` plus the recorded UVI diagnostic-only edits;
all 1,168 captured Rust/Lua/manifest/lock source hashes stayed unchanged during
its locked, offline corpus build. Attempts to acquire a compiler-free
measurement window stopped at the passive compiler gate before launching the
fixture. Consequently there is no new callback baseline, current exclusive
stage attribution, accepted speedup or 1% result from this frozen executable.

The bounded candidate moves the existing 512-byte zeroed scratch initialization
inside the `points.is_some()` branch that reads it. Direct voices already write
to their destination and do not use scratch. Rust definite assignment keeps the
initialized array on the ramped path without unsafe storage, heap allocation or
any change to sample arithmetic, control cadence, source eligibility or DSP
history. This does not implement a broader no-chain render bypass.

A new test-only authored fixture lowers 100 keys with three nonoverlapping MIDI
velocity layers each: 300 independently allocated assets, selected samples,
physical voices and exact host owners. Whole 256-frame renders and
1/7/64/65/119-frame fragments produce identical PCM bits and cursor/envelope
continuation across source wraps; every exact owner releases and runtime
fault counters remain zero. It passes before and after the candidate. This is
authored core-selection/state evidence, not an exported-host or actual callback
meter/heap witness. The existing callback fixture retains those separate guards.

The focused render tests pass. The core functional source manifests contain
284 files; only `render.rs` differs between candidate and original. Optimized
before/after disassembly proves the 512-byte `memset` moved behind the points
discriminant and the direct branch skips it. The `render_voice` symbol shrinks
from 7,205 to 7,192 machine-code bytes. Static instruction removal is not an
elapsed-time measurement. Same-cohort, compiler-free actual callback pairs and
their complete PCM/heap/counter checks remain required before timing promotion.

The seven relevant integration targets pass: EQ lane/processor history, script
fade curves, paged fallback, realtime heap guards, sample revisions, source
playback and voice modulation (64 passed, one preexisting ignored test). The
candidate source hashes stay unchanged throughout these functional checks.

Raw functional logs, source manifests, assembly, passive compiler-gate records
and the frozen baseline are in
`/mnt/Windows11/DEV_WORKSPACE/PluginArtifacts/kontra-cpu-20261015-render/`.
