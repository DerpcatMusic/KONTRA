# Native summed-signal DSP

`Prepared::with_buses` binds regions to a prepared stereo bus DAG. A `None` region
binding remains direct output. Each bus owns an ordered processor list, explicit
post-processing sends and a maximum zero-input tail in output frames. Each send
targets another bus or stereo output with a finite linear gain. Source translators
can preserve serial and parallel topology; a separate tap node represents a send
before a later insert. Nothing moves a per-voice effect across a summing boundary.

This implements a shared bus scope within architecture §8.5. It is not complete
Kontakt/Falcon graph import, proprietary processing or arbitrary channel routing.
Voice/family scopes and frontend program/layer/keygroup semantics remain distinct.

## Preparation and state ownership

Preparation validates every target and gain, rejects cycles (including disconnected
or zero-gain cycles), and builds a deterministic topological schedule. Node indices
remain source-binding indices; authored nodes need not be topologically ordered.
Processors use the same native `Processor` definitions, coefficient preparation and
finite/denormal handling as voice chains. The former `VoiceProcessor` name is
replaced directly; there is no parallel processor implementation or compatibility
alias. Bus filter rates and stable control identities must match the plan.

A retained generation owns one history bank for each bus, bounded stereo scratch
buffers and sample-clock gain trajectories. Voice histories remain per voice.
All banks allocate on control and move through the existing plan adoption/retirement
exchange. Bus automation and voice automation read the same headless control value,
with independent declared mappings/ramps. Builder order cannot discard another
scope's control bindings, and schema replacement revalidates both scopes.

Rendering splits scheduled segments into bounded 64-frame scratch blocks only when
a retained plan has bus routing. Voices sum into their assigned inputs in voice slot
order; each bus runs once after all of its predecessors. The direct-only rendering
path is retained. The scratch size is not a control-rate substitution: processor
and gain evaluation remain per sample on absolute time. No bus processing adds
buffered latency, and no future automation values are read early.

## Tails, cleanup and containment

Source/voice processors report their exact produced frame count. A bus starts its
zero-input budget after its last contributing frame, including upstream voice/bus
tails. Silent active sources continue to advance state; tail duration is not inferred
from a threshold or host block size. A truncated bus resets its history when it
becomes inactive. Shared tails do not pin contributing note IDs: host terminal
delivery and note-slot reuse can complete while the bus still sounds.

A bus tail independently prevents its generation from retiring. Old and new plans
can render together with separate controls/histories; completed plans still return
to control for destruction. Global panic clears all bus histories and tail budgets.
Channel sound-off cannot selectively erase a mixed shared tail without also changing
other contributors; domain-specific routing/cleanup profiles remain frontend work.

A nonfinite/unrepresentable bus output or poisoned state in a block (up to 64 frames)
clears that bus's processor history and suppresses that bus block, incrementing the
observable fault counter once.
Other bus histories continue. The final output retains the existing finite-sum guard.
The native graph rejects feedback: feedback delays, latency compensation, multichannel
layouts, dynamic routing and family-specific graphs remain required extensions.
Nonlinear/oversampled processors and automated filters remain open; current processors
are gain, matrix, prepared biquad and control-driven gain.

## Evidence

`sampler-core/tests/buses.rs` uses independently evaluated impulse recurrences and
convolution, exact control trajectories and the heap guard. It covers summed voices,
non-topological authored order, serial/fan-out sends, whole/split blocks through 129
frames, source/voice/bus tail boundaries, host note retirement before bus completion,
old-generation tails alongside new audio, shared control values at distinct scopes,
invalid graphs/rates/identities, fault containment and panic/reset reuse.

`render_workloads --bus-filters COUNT` measures a summed bus separately from the
existing per-voice `--filters` workload. Both validate every measured output block.
These are different authored scopes; lower bus cost does not justify moving effects
from individual voices or claim performance superiority over Kontakt/Falcon.

Validation: 339 native tests pass in release and Rust 1.92; strict all-target Clippy
and both root boundary tests pass. Logs and the local workload CSV use
`artifacts/bus-dsp-*`. One unpinned local run at 48 kHz, 1,024 voices, four summed
bus filters and 64-frame blocks measured 31.811 μs median and 59.551 μs p99 across
512 checked callbacks. At 256 frames: 128.193/228.914 μs. This constant-input workload
has no proprietary DSP, active modulation or storage pressure; it is a local baseline,
not a worst-case deadline or competitor comparison. Broader product conformance, proprietary DSP matching and production host/UI wiring remain open.

## Bus reverb and convolution

Bus processors include an algorithmic stereo `Reverb` (8-line FDN, v1's design) and a
`Convolution` (`dry * x + wet * (x * impulse)`). Both are bus-scope only: a voice or
group chain that contains one is refused (`Feature::VoiceReverb`). The convolver is a
zero-latency non-uniform partitioned FFT (a 64-frame direct head, then stages of
growing partitions up to 8,192 frames); the spectral multiply-add dispatches to the
CPU's widest level. Impulse responses live in `ir::Instrument::impulses`, are shaped
by the importing profile (reverse, predelay, volume envelope, auto gain), resampled to
the output rate at lowering (Blackman-windowed sinc) and held by the plan
(`Prepared::with_impulses`); allocation happens at runtime construction only. A
convolution bus's tail is the response length plus two largest stage blocks.

Kontakt translation (`sampler-kontakt/src/effects.rs`) maps the instrument insert,
send and main racks to buses. Unmodelled parameters are listed in the load report by
module and parameter: IR size, early/late filtering and decimation, reverb and EQ laws
(`UnknownLaw`), and every other module (`NotModeled`). Bus (`InsertBus`) racks and
group-insert Filter slots are still report-only.
