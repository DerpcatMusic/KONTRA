# Native envelopes and release ownership

The clean-sheet core now renders native linear attack/hold/decay/sustain/release
from validated per-region `Envelope` values. Frame durations are at the prepared
sample rate; sustain must be finite and in [0, 1]. No legacy envelope code or
vendor parameter interpretation is used. The default remains constant unity
with immediate release for raw PCM playback.

At source start, attack begins at zero. Attack reaches one at its exclusive end;
hold stays at one; decay reaches sustain at its exclusive end. Zero stages are
skipped. A gate closure captures the next held sample's level, then ramps it to
zero over the release duration. Repeated cleanup never restarts that ramp.
Phase progress uses bounded integer frame counts; ramp calculations use f64 and
produce f32 gain. There is no block-size-dependent accumulation of ramp error.

A gate closing seals the family's admissions and cancels pending source starts.
Already sounding sources continue through their release. Source EOF can finish
earlier. The family retains the note, and the note retains its expression owner,
until every tail ends. Only then can an accepted terminal notification release
the logical identity. Pedals act on the effective gate, so a sustained key-up
does not begin release. Explicit voice/family stops and panic remain hard stops.
These operations require no queue capacity or callback heap work.

## Timed family choking

`choke_family(family, frames)` seals that family's admissions, cancels its delayed
starts and fades each sounding source from its next envelope sample's level. It
uses the existing linear release state; no extra per-voice state or render branch
is added. An already releasing source keeps its trajectory if its remaining tail
is shorter than the requested duration. A shorter choke captures the current level
and reaches zero at the new exclusive end. Repeated commands cannot prolong tails.
Zero frames uses the same hard-stop path as `stop_family`.

Choking does not close a logical gate, consume a physical key, cancel note behaviors
or affect sibling families. It also does not reset playback or exit an until-release
loop: the source continues beneath the fade, unless a later gate closure changes
that loop. Source EOF may finish earlier. Tail ownership and terminal retry use the
normal family/note path; a faded-out physical note still waits for its key-up.

`Event::ChokeFamily(family, frames)` schedules the same operation in sample time.
Equal-time source starts and chokes follow submission order: an earlier choke
cancels an unstarted source; an earlier start is sounding when the choke captures
its level. Immediate choking works with a full queue; future scheduling can return
capacity without modifying the family. Stale/foreign targets fail admission.

A future choke does not pin the family or its note. If the family ends naturally,
the queued command is discarded by subsequent cleanup or becomes a no-op when due.
Its full generational identity prevents it targeting a replacement slot. This is a
native choke primitive, not automatic victim selection, voice-stealing reserves,
exclusive-group mapping or a vendor fade-curve interpretation.

Six heap-audited tests in `tests/choke.rs` independently check per-layer attack and
release levels, unequal envelope phases, siblings, full queues, delayed/equal-time
starts, natural EOF, stale/reused/foreign IDs, loop phase, silent advancement and
physical/terminal ownership. Regular, irregular and zero-size render partitions
agree exactly. Mixed-operation ownership invariants now also exercise timed chokes.
All four native crates pass debug/release/MSRV tests and strict all-target Clippy;
the root historical boundary tests remain a separate validation check.

`start_family` now takes an explicit envelope for manually admitted sources;
`trigger` uses each prepared region's envelope. The single-source `start`
convenience uses the constant envelope. The native demo exercises a 5 ms attack,
100 ms decay, 0.8 sustain and 50 ms release after pedal-up at one second. Raw WAV
rendering uses the constant envelope and preserves decoded samples.

## Ownership review following Rust Doctor

Reviewed callback invariants in `lib.rs`, `ownership.rs`, `schedule.rs`, `gate.rs`
and the prepared-selection boundary before adding tails:

| Private access | Evidence and continued obligation |
| --- | --- |
| Arena index/generation | External handles use checked index, runtime and generation matching. Internal slot IDs come from bounded arena loops. Exhausted generations are quarantined. |
| Voice -> family -> note -> expression | Voice/family/reference counts prevent parents retiring early. Tails use the same count path, not separate borrowed lifetimes. Mixed-operation ownership tests cross-check reachable counts. |
| Parent note during child admission | Public child admission validates the parent before entering the private admission function. There are no concurrent writers or user callbacks between validation and insertion. Descendants prevent parent retirement. |
| Scheduled source and note accesses | Reserved voices remain alive until start or cancellation. Expression commands own separate work pins. Closed-note cleanup removes commands before note reclamation; public unpin cannot consume work pins. |
| Prepared candidate/sample indices | Control-thread construction validates samples/ranges and owns immutable buffers. Trigger checks key and velocity before indexing; preflight admits all layers transactionally. |
| Render slices | Event boundaries are between current time and checked block end; source cursor advances by min(segment length, remaining PCM). Output and source slice endpoints stay within their owners. |
| Envelope counters | Durations are u32, stage sums/age use u64; held age stops at the sum. Release increments only below its duration. Zero duration retires without division. |

These are explicit invariants, not blanket lint exemptions. The public error
boundaries, stale-ID/pressure tests and generated mixed-operation test remain
necessary. WAV parser findings and broader API documentation still need their
own review; this is not a claim that all Rust Doctor findings are resolved.

## Executable evidence

`crates/sampler-core/tests/envelope.rs` checks an independently specified 18-frame
AHDSR sequence under five block partitions, including empty renders; release in
the middle of attack; owner retention and terminal rejection; repeated cleanup;
pedal release, panic, cancellation before source start; invalid sustain values,
maximum durations, zero release and source EOF. Two tests instrument allocation
**and deallocation** across the complete tail lifecycle.

The existing ownership, scheduling and prepared-selection suites continue to
pass. All 25 new-crate tests pass in debug, shipping release and Rust 1.92.0;
strict all-target Clippy and the full root CI-profile test command pass.
The complete Rust Doctor baseline gate passes with no new error-level findings;
three new warning fingerprints remain for reviewed render complexity/indexing
and the changed prepared-admission expect site. The rendered demo contains 96,000 stereo frames,
a nonzero tail after frame 48,000, and exact zeros from frame 50,400 onward. The envelope test supplements them rather than replacing their saturation
and independent reference checks. Local logs and rendered evidence are retained
under ignored `artifacts/architecture-v2/envelope-*` paths.

The constant-envelope resident benchmark remains at approximately 0.39 us,
2.35 us and 5.32 us median for (16 voices / 64 capacity), (16 / 4096), (256 / 256),
64-frame blocks at 48 kHz on this machine. All checksums match the prior slice;
no deadlines were missed in this run. This only measures the unity fast path,
not streaming or a complete plugin. With `--envelope`, all voices remain in
attack for the entire measured run: medians 1.671 / 3.350 / 23.350 us,
p99 4.280 / 5.330 / 38.020 us, maxima 8.300 / 22.740 / 54.031 us, zero deadline
misses for the same three workloads. These are uncontrolled local measurements,
not a worst-case execution-time proof.

Still open: vendor curve/clock profiles, tempo-relative durations, live envelope modulation,
repedaling, automatic stealing/reserves, release-trigger mapping, and audio-host
integration. The native linear envelope is not a compatibility interpretation
of Kontakt/HISE/Falcon or other engines.

## Region velocity response

`Prepared::with_velocity_curves` assigns constant, linear or positive-power
amplitude response in authored region order. Defaults remain linear. Curves shape
voice gain once at admission, not raw note velocity, layer predicates, expression
state or the render loop. Attack, physical-key release and effective-gate release
all use the same admission path, with each release phase's configured velocity.

The preparation boundary rejects invalid exponents and mismatched region counts.
The heap-audited release-selection fixture covers zero/full/high-resolution input,
independent curves, raw-velocity threshold selection, natural EOF and retirement.
This is a native primitive, not a claim about Kontakt/Falcon curve parameter units
or their complete velocity modulation behavior.

Velocity-response validation: all 195 native debug tests and strict all-target
Clippy pass (`artifacts/velocity-response-{debug,clippy}.log`).

## Curved DAHDSR and one-shot AHD

`Envelope::with_delay` adds an output-frame delay; sample playback continues under
that silent stage. `with_curves` independently prepares attack, decay and release
shapes. `EnvelopeCurve::exponential(k)` uses the native normalized shape
`expm1(k*t)/expm1(k)`, with `k=0` exactly linear and finite `k` in [-32,32]. Positive
values start slowly, negative values start quickly. These units are deliberately
not a Kontakt/Falcon parameter interpretation.

Curve coefficients are prepared off audio. Rendering advances an f64 difference
recurrence, anchored to its absolute stage age every 64 samples rather than host
blocks. The near-zero formula uses the continuous limit of `expm1(x)/x`, including
subnormal curvature inputs. There is no per-sample power operation and no heap work.
Stage endpoints are explicit; no approximate asymptote decides completion. Linear
stages retain their original arithmetic. Constant sustain and unity paths remain.

`Envelope::one_shot` runs AHD to zero independently of key-up and retires its source
at the exclusive decay endpoint. It can also have a delay and stage curves. A
physical key still retains its logical identity after AHD completion. Panic, explicit
stops and family chokes retain authority; a choke captures the next curved sample
and uses the existing linear fade, never extending a finite AHD or release tail.
Envelope gating and source-loop exit policy remain distinct.

The analytic fixture checks all stage/release boundaries, independent curvature
signs/extremes and near-zero inputs across several regular/irregular block sizes,
including empty renders. Additional heap-audited cases cover one-shot early gate
closure, zero-duration stages, held-key retention, curved-release choking and exact
retirement. Private clock checks reach u32::MAX boundaries without iterating billions
of samples. These establish native semantics, not vendor fidelity.

Kontakt's [modulation manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation)
describes an attack-curve control, AHD-only mode and flexible envelopes. The [UVI
parameter reference](https://lua.uvi.net/_elements.html) lists independent DAHDSR
stage curves, delay and trigger modes. The older implementations contain distinct
control-clock and parameter mappings. Matching those profiles, including retrigger,
shared scope and control-rate interpolation, remains required; the new native shape
must not silently replace them under an equivalence claim.

Curved-envelope validation: all 199 native tests pass in debug, release and Rust
1.92, strict all-target Clippy passes, and both root boundary tests pass. Logs use
`artifacts/curved-envelopes-{debug,release,msrv,clippy,boundary}.log`. The existing
resident render workload also completes with its exact PCM assertions; its current
CSV is `artifacts/curved-envelopes-workload.csv`. That workload checks the retained
constant/sustain fast paths, not curve cost or comparative vendor performance.
