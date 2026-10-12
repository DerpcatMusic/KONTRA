# Ordinary sampler counted and ordered loops

This trace applies to the installed Kontakt 8.13.1 PE64 executable, image base
`0x140000000`, SHA-256
`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.
Addresses below identify bounded disassembly/decompilation of the actual sampler
consumer, rather than an inference from the similarly named Beat Machine script.
The executable is at
`/home/derpcat/.wine/drive_c/Program Files/Common Files/VST3/Portapotty/Kontakt 8/x64/Kontakt 8.exe`.

## Record to ordinary sampler consumer

The `BLoop` RTTI descriptor is at `0x14a3a9d30`, vtable `0x144e91240`.
Reader `0x140cff510`, version `0x60`, converts saved modes `0/1/2/3` to
runtime modes `0/3/4/1`: inactive, Until End, Until Release, and One Shot.
It copies the signed saved count verbatim to record `+0x18`; start/length
are `+0x08/+0x0c`, alternating `+0x1c`, runtime mode `+0x20`, tuning
`+0x24`, and crossfade `+0x28`. Writer `0x140d0faa0` reverses that conversion.

Voice preparation `0x140968ca0` installs the zone's eight-row loop array
(`zone+0x7d8`, row stride `0x38`) at `voice+0x560`, initializes the physical
integer/fraction cursor at `+0x164/+0x168`, and calls selector `0x140960c00`.
Selector traversal is ascending, skips inactive rows, and copies a selected
row's count to `voice+0x174` (`0x140960cf7..0x140960cfa`). It resets
`voice+0x178`, the first-boundary marker, when selecting a new row. For ordinary
loop modes it skips a row if the current integer cursor is already at or beyond
that row's end. Selection does not jump the physical cursor to a later loop's
start and does not prorate the count when starting inside a loop.

Forward geometry helper `0x140880860` clamps start to `[0, sampleFrames-4]`,
clamps length against `sampleFrames-rawStart`, enforces a minimum of four
frames, then adds start and length. Selector rejects rows beginning beyond the
effective cropped sample end. This is separate from the saved array's order.

`SamplerModule` RTTI is at `0x14a3bc7d0`, vtable `0x144fd21e8`.
Its consumer at `0x140d206a0` obtains loop-control buffers and tail-jumps
to ordinary sampler producer `0x14087ab10` at `0x140d2073e`.

## Count, overshoot, and release

At an end crossing, producer `0x14087b101..0x14087b11c` marks the first
boundary, decrements a positive remaining count **before deciding to wrap**,
and selects the next row when that decrement reaches zero. Therefore count
`1` gives no extra repeat, `2` gives one repeat, and `0` repeats indefinitely.
The same first end crossing consumes one count when the initial cursor starts
partway through the loop. Negative counts also follow the unlimited branch in
this consumer without decrement: the exact signed `jle` at
`0x14087b110` branches to the wrap path. They must remain signed and must not
accidentally cast to a positive budget.

Forward wrap at `0x14087b245..0x14087b25c` subtracts the current end and adds
the current start to the physical integer cursor. It leaves the fraction intact.
On exhaustion, `0x14087b157..0x14087b16a` selects the next row without rewriting
the cursor. Consequently a large source step can overshoot and skip a later
short loop: a pitch-independent unrolled timeline cannot express every ordered
case. Repeated end crossings must consume the same per-voice budget during
audible rendering and muted advancement.

The producer's host-fragment end exit at `0x14087b29f` precedes boundary
consumption. The committed cursor therefore retains a pending physical crossing
until the next output point. A release delivered at that boundary can set the
current Until Release budget to one before normalization; eagerly wrapping at
fragment commit would incorrectly add another pass.

Scheduled release calls `0x14086aea0` from `0x140906926`. Release clears held
state at `voice+0x504`, then forces the current Until Release loop's remaining
count to `1` (`0x14086afd9`) without moving its cursor. Its final forward pass
continues to the end; subsequent selection receives `skipRelease=true` and
skips later Until Release rows while retaining later Until End rows. An
alternating loop traveling backward instead receives `2` at `0x14086aff5`;
that direction-specific trajectory is outside the initial forward slice.

## Interpolation is phase dependent

Producer `0x14087af1b..0x14087af32` emits two distinct wrap flags. Bit `0`
reflects `voice+0x178` (a previous boundary); bit `1` permits another repeat
when remaining is nonnegative and not `1`. Four-tap resampler `0x14086e030`
maps the current floor cursor minus one, floor, plus one and plus two through
helper `0x1408ed780`.

For nonalternating loops that helper uses **bit 0 only for taps below the loop
start**, and **bit 1 only for taps at or beyond the loop end**. Thus the final
pass after a repeat still wraps its left stencil, while its right stencil reads
the physical tail. Count `1` on the initial pass uses the physical pre-loop
left stencil. This closes and retracts the earlier provisional concern that
remaining `1` disabled both stencil directions. A single finite virtual loop
can model this seam; the ordered high-step selection boundary still needs the
actual physical cursor and active-row state.

Negative remaining counts differ from zero at the right stencil despite their
shared unlimited trajectory. The signed `js` at `0x14087af2b` suppresses bit 1
for negative values, while zero reaches the `or al,2` at `0x14087af32`. Thus a
negative loop reads physical right-tail taps before every wrap, then wraps its
left stencil after the first crossing. Release still writes signed count one;
no sign-dependent release exception occurs. The prepared row and scalar cursor
retain signed counts so this distinction survives all RAM and sparse-stream
tap paths, advancement, and release.

Crossfade helper `0x140889570` has no remaining-count-`1` exclusion and can
override the producer's flags. It is therefore not valid to infer that the
final pass disables crossfade. Fractional counted crossfade, counted alternating playback,
nonunity loop tuning, and One Shot interactions require separate consumer
proof before admission.

### Forward crossfade consumer and integral admission

The producer clamps width to `max(0,min(savedCrossfade,loopLength-1,loopStart))`
at `0x14087af5c..0x14087af7f`. Positive width unconditionally sets stencil
flags to `2` and the extra crossfade marker at `0x14087af81..0x14087af8a`, then
sets fade begin/end to `max(loopEnd-width,loopStart+1)` / `loopEnd`. These
stores occur after the signed-count right-stencil check and override it even
for remaining count one or a negative count. In the resampler, that extra
marker excludes right `+1/+2` taps beyond the end (`0x14086e030`, branches
corresponding to the loop descriptor's byte `+10`). Thus a final-pass fade
cannot be removed merely because another physical wrap is disallowed.

Source loader `0x140828500` applies a linear gain to decoded physical samples
**before** interpolation. Loader fields `+0x14c/+0x150` alias voice fade
begin/end `+0x154/+0x158`, because the producer supplies `voice+8`. Own gain
initialization at `0x140829298..0x1408292cb` uses `-1/width`, then
`1+(physicalBlockStart-fadeBegin)*step`; its loop adds that float step once
per decoded sample at `0x1408292ef..0x1408292fc`, with zero/one clipping.
The partner branch `0x140829332..0x14082938c` uses `+1/width` and starts from
`(physicalBlockStart-fadeBegin)*step`. Literal one is `0x14470bad4`; minus
one is `0x144714a54`, loaded at `0x140828b84/0x140828bc7`.

Helper `0x140889570` subtracts loop length from the partner cursor and both
fade bounds (`0x140889638..0x140889687`), enables the partner-gain branch via
voice `+0x141`, resamples it separately through `0x140865c70`, then adds the
two already-interpolated output streams. It restores the bounds and marker
afterward. Its initial gate at `0x14088960a..0x140889611` compares the
post-main integer cursor to fade begin; this is a fragment boundary, rather
than an established per-output-point gate. Loader gain accumulation is based
on a 512-frame physical block. Fractional stencil support around fade begin and
native caller fragment boundaries therefore remain required proof for generic
counted-crossfade admission. The loader rounding itself is explicit: initialize
from the physical 512-frame block origin and repeat float addition once for each
preceding sample in that block, with no algebraic replacement by a direct
tap/width division. A single weighted output or a count-one fade bypass would
not implement the traced consumer.

The fragment gate has a concrete consequence derivable from these instructions.
For loop `256..1024`, width `128`, constant source one, physical phase `895.5`,
and source step `0.25`, the main weighted quartet is `[1,1,1,127/128]` and
the shifted partner quartet is `[0,0,0,1/128]`. Resampler `0x140947580` uses
the verified coefficient vectors one / `1/6` / `1/2` at
`0x144714b70` / `0x144f39230` / `0x144f39250`; at the midpoint the main
value is `1+1/2048` and the partner is `-1/2048`. A one-output request ends
at `895.75` (integer cursor `895`), so the helper gate excludes the partner.
A two-output request ends at `896`, so it includes the partner for the whole
segment and the first output sums to one. All gains in this example have exact
binary reciprocals; it does not depend on gain accumulation error. This is an
instruction-derived prediction, not a captured Kontakt PCM measurement.
The ordinary sampler call's requested fragment and a physical 512-frame decoded
gain block are distinct boundaries. Their caller quantum must not be silently
identified with KONTRA's host fragments or 32-frame control points. At integral
source phases, a pre-fade partner's center sample is zero, so this particular
fractional support ambiguity disappears independently of partition.

The bounded caller chain distinguishes those boundaries further. The static
`SamplerModule` object at `0x149e84698` has vtable `0x144fd21e8`; entry `+8`
is `0x140d206a0`, which forwards to sampler producer `0x14087ab10`.
Dispatcher `0x140916350` calls that entry at `0x140916d42`, passing its
requested output count minus the voice's pending start delay (`+0x170`),
without a 512-output split. Its caller `0x1409462c0` loads that count from
synth `+0x129c0` at `0x1409467dc..0x140946811`. Synth process
`0x140906000` stores the field directly from its `r8d` count argument at
`0x140906154`.

The next caller is `BInstrument::process`, identified by the
`NI::AB::IInstrument` base and vtable `0x144fd37b0`. Entry `+0x18`
(`0x140d38fc0`) advances the request pointer by `0x48` before forwarding to
`0x140d38620`; that function reads count at the resulting request `+0x1c`
(full interface request `+0x64`). For counts at most 1024, it passes the
whole count through to synth process at `0x140d38b88`. Larger counts use
`k=ceil(count/1024)` balanced chunks of `ceil(count/k)`, capped at 1024,
with the last chunk limited to the remainder (`0x140d38b92..0x140d38d3b`).
Thus the traced output producer chain is request sized, distinct from the
physical decoded-gain block. The outer interface request's framing remains
unproved; these callers do not justify a fixed 512-output clock or a
per-output replacement for the native post-main helper gate.

Fractional source interpolation also has a separate traced law. Boundary
resampler `0x140947580` and regular decoded-run resampler `0x140946ca0`
implement four-point Lagrange interpolation. With fraction `t`, their weights
are `-t*(t-1)*(t-2)/6`, `(t+1)*(t-1)*(t-2)/2`,
`-(t+1)*t*(t-2)/2`, and `(t+1)*t*(t-1)/6` for the four physical taps.
The native constants are float bits `0x3e2aaaab` (`1/6`) and `0x3f000000`
(`1/2`). This agrees with the Compatibility Hermite kernel at integral phases and
the midpoint, but does not justify reusing that kernel for other fractions.
Native multiplication/addition order and phase accumulation remain relevant
to any stronger numeric-parity claim.

Consumer `0x140865c70` selects a per-output pitch-factor buffer at synth
`+0x1030+slot*8`, offset by the already-produced output count, when the
slot flag at `+0x10f0+slot` is active. Regular constant-step kernel
`0x140946ca0` initializes four phases through successive float additions,
then advances each lane by float `4*step`. Curved kernel `0x140946f60`
multiplies each buffer factor by the base step and accumulates those float
steps. Helper `0x140889570` therefore receives the actual post-main segment
phase; current-step times requested-count is not a proven substitute for a
curved or externally retuned trajectory. A shared outer host-window boundary
can establish the requested count, but does not by itself establish those
future source phases.

## Implementation boundary

The implementation scope is ordinary forward sampling with unity loop tuning,
nonalternating ordered rows, and signed saved counts. Effective zero-crossfade
rows retain fractional playback. Positive effective width is admitted only
when every actual source phase and its pending fragment endpoint are integral;
initial or live-retuned fractional phases fail before PCM or retained source
clock mutation. This preserves the proven integral source behavior without
assigning KONTRA's local fragment boundaries to the untraced native caller.
Saved negative widths and positive widths clamped to zero remain ordinary
fractional rows. Raw saved metadata is preserved in every case.
Prepared rows belong to the bank; per-voice scalar state owns the physical
cursor, active row, remaining count, first-boundary marker and release state.
The existing source/envelope clocks remain authoritative. RAM and streaming
must receive the same phase-derived physical tap coordinates; streaming reuses
its existing sparse decoder and packet cache. Unsupported active rows must be
retained and diagnosed, never silently replaced by a later supported row.

A crossfaded prepared trajectory requests two physical quartets per output,
within the existing eight-tap-per-point sparse packet bound. The integral
renderer applies exact loader gain to each raw center, then adds the two
independently resampled centers. This is the integral specialization of the
traced pre-interpolation gains, not a weighted output approximation. Own and
partner decoded gains have separate rounding histories: their sum need not
equal one. Gain reconstruction repeats at most 511 float additions from the
physical decoded block origin. Two local render caches retain the unclipped raw
gain and continue the same additions for monotonic taps in that block. A changed
fade, stream side, block, or backward tap regenerates the original prefix;
clipping applies only to the returned gain. Reuse therefore preserves every
native float addition and introduces no heap or persistent voice state. It never
substitutes direct division or changes the audio according to CPU load.
Noncrossfaded trajectories keep four-tap
requests and their existing interpolation path.

Saved-NKI/cache/PCM fixtures include widths 128/256 and nonbinary 127/300/3,
the last two exercising native start/length clamps. An independent whole
512-frame gain-buffer reference and literal float-bit landmarks at the
physical block1024 boundary verify the per-tap prefix reconstruction, including
count one, negative count, UntilRelease final fade, ordered rows, streaming,
host partitions, mute advancement, and finite endpoints. The fixtures also
stage checked initial/live-retune failures where the only requested output
point is integral but its pending endpoint is fractional. These checks are
staged; the implementing agent did not execute Cargo or tests.

Admission rejects a loop end beyond the cropped sample end, including native
negative-start geometry that would extend beyond the source. This is narrower
than the native selector's start-only cropped-end gate and is diagnosed. Raw
geometry and sparse physical slot identity still round-trip unchanged.

The existing one-millisecond sample-end ramp follows actual future source-clock
points and loop selection; nominal later repeat lengths are not an EOF oracle
because overshoot may skip those rows. The preview returns terminal source
travel, subtracting the physical terminal overshoot. Each actual output divides
the remaining source distance by its current source increment, preserving the
ordinary finite-source fade law under curved pitch rather than replacing it
with a future output-time ramp. Its prepared lookahead admits at most six
128-point fade blocks, one float-rounding point, the current block and endpoint
(covering ordinary rates through 768 kHz). Larger/invalid counted-loop clocks fail explicitly before PCM; the
fade is never shortened. This fixed representation bound does not depend on CPU
load. A currently unlimited row needs no EOF preview until release makes its
budget finite. Nonzero live loop geometry controls also fail before PCM rather
than silently changing or ignoring the prepared rows.

The ordinary single unlimited forward zero-crossfade Until Release fast map
retains only wraps already consumed at the actual fractional cursor. Its wrap
budget uses the ceiling of that cursor: an exact pending end crossing consumes
no extra pass, while a fractional position just beyond the end retains the pass
already entered. The prior three-frame stencil lookahead is retained for other
crossfade/direction modes pending their separate proof. Saved NKI PCM cases at
integer and half-frame positions before, exactly at, and after the first end
check RAM, streamed packets, fragmentation, and independently written finite
physical trajectories.

Release-triggered local voices retain the existing engine policy: initially
select a row, then apply the already-up release state. Native preparation's
unconditional `selector(voice,0,false)` is established; the complete native
generated-voice pending-release scheduling is not claimed by this loop slice.

The [Kontakt manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view)
corroborates unlimited count zero and the distinction between Until End and
Until Release. Its prose does not establish whether count one is an extra
repeat; that boundary above is established by the sampler consumer.

## Selected ordinary sampler interpolation

The saved public Group interpolation field is now connected to the native
runtime selection, rather than inferred from the cubic's name. BGroup's
Serializable vtable `0x144e97ab0` has RTTI subobject offset `+0x10`
(`0x14989f198`). Public reader `0x140d0cf80`, for versions `0x90..0x96`,
reads the final signed quality integer through `0x140cf0360` into receiver
`+0xc8`: full BGroup `+0xd8`. The reader accepts exactly 0, 1, and 2.
XML reader `0x140920cd2` maps `interpQuality` values `normal`, `high`, and
`ultra` to those same integers at `0x140920d17`, `0x140920d48`, and
`0x140920d87`. This matches the retained archive field `interp_quality`.

Voice initializer `0x140968ca0` maps group quality zero to null tables at
voice `+0x3a0/+0x3a8`; qualities one and two select distinct nonnull tables.
Ordinary sampler consumer `0x140865c70` selects `0x14086e030` for the null
case, which dispatches to the Lagrange quartet kernels traced above. The
native global override's image initializer is 3 (inherit group); this slice
does not claim parity with an external high-quality/offline override.
WavetableModule process `0x140d20110` instead forwards to `0x14087bf50` and
its separate `0x140cddeb0` generator. Its interpolation law is not established
by the ordinary sampler chain.

KONTRA therefore selects Lagrange only for Native-policy ordinary Sampler
identity `source_mode == Some(0)` with saved quality zero. Compatibility,
opaque source identities, Wavetable, and the unimplemented native quality-one
and quality-two table kernels retain their prior paths. The selected identity
occupies previously unused packed-lane bit 61, participates in lane equality
and hashing, and chooses matching filter-adjoint weights. Unity tap paths and
all source/envelope/LFO clocks are unchanged. Same-record residency fills keep
the selected kernel; changed source mode or saved quality now replace the
instrument state and close its old voices. This implements the selected
polynomial, not complete bit parity with native float phase grouping or every
native regular/boundary multiplication ordering. Fractional crossfade still
requires the separately unresolved future producer phases and helper gate.

Staged independent checks use node-basis products evaluated in double
precision, a literal quarter-phase basis value `-5/128` (Compatibility
`-3/128`), and cubic reproduction. Complete PCM compares RAM, streamed,
wide-window, ordinary-loop, shared/solo-filter, and preserved-quality paths
against a separately resampled unity source, plus same-record fill and changed
identity lifetimes. The counted curved-tail unrolled oracle now loads its own
saved Native NKI so its cubic policy matches the counted source. These checks
are staged; this agent did not execute Cargo or tests.

## Higher-quality ordinary sampler tables and forward source admission

The nonnull-table ordinary consumer is `0x14086eb50`, reached from
`0x140865c70`. Descriptor constructors `0x140456f10/f40/f70/fa0` store
integer density 128, base extent 9 for High and 25 for Ultra, and table support
512 and 1536 respectively. The four descriptor/table pairs are High up
`0x14a5c5d60/0x14a5c5d80`, High down `0x14a5c1cc0/0x14a5bfc80`, Ultra up
`0x14a5b9a18/0x14a5bbc40`, and Ultra down `0x14a5c7f10/0x14a5c1d00`.
These are ordinary sampler tables; the separate Wavetable generator has not
been shown to use them.

Preparation thunks `0x140457de0/e00/e20/e40` call builders `0x1406ffd50`
and `0x1407000f0`. Both up tables use float cutoff 1.03 and beta 9; High down
uses 0.82 and 6.6, Ultra down 0.9 and 5.6. The builders set coefficient zero
to the cutoff and compute subsequent coefficients from the vector sine of
`cutoff * j * pi/128`, divided by `j * pi/128`, rounded to float. A double
precision Kaiser I0 series forms the window weight. The normalized weight is
rounded to float before multiplying the float sinc coefficient, with a second
float rounding (`0x1406fff49/4d`). Its
window parameter is `sqrt(max(1-j*j/(support*support),0))`, evaluated through
its square in the series; the denominator is I0(beta), and each series
continues while its added term is at least `1e-21`. Stored float pairs contain
the coefficient and the next-minus-current coefficient. No per-phase sum
normalization was observed. CPU-dispatched vector sine `0x1443c9bb0` remains
part of the generation contract. Isolated Unicorn execution of the two pinned
builders and their actual sine dispatches, with dispatch flags 1 and 2, produced
identical complete buffers in all four cases. A scalar reconstruction using
the evidenced intermediate rounding and precomputed series factors matched
all 8208 generated float entries on this analysis host. This establishes the
table boundary here; it does not claim every platform's scalar sine shares the
same rounding. The complete little-endian output hashes (including the final
zero pair) are:

| Table | SHA-256 |
| --- | --- |
| High up | `9a191034f98e6eb2e53d8dd0fb00ef1c986062b34e3a4e326e921ebe4b7fc1e4` |
| High down | `2abedf75fde100a5c0de7aa77acbdce66afd86a0632088bb1e202dd8cb4f3d55` |
| Ultra up | `05353f4a054f7faa9c31799d8382b8c7dd54fb24deabe51a4cefbd9cab5f145a` |
| Ultra down | `60d857f29a3c896ae60b9c4560e3dc046e00c288cc51825ab4b7e08c681855c6` |

At source step at most one, the consumer uses the up table, table distance
scale 128, and output gain one. Above one it uses the down table, float scale
`128/step`, and float output gain `1/step`. This expands source support with
pitch: nominal radius is four or twelve source frames times the step. Integer
steps above one still convolve. The upstream static near-unity shortcut at
`0x14087ab10` uses `abs(step-1) <= float(1e-4)`; an active pitch-factor buffer
forces resampling regardless of that comparison.

For the ordinary `voice context+0x138 == 0` branch, coefficient helper `0x140828350`
starts at float `fraction*scale` on the left and `(1-fraction)*scale` on the
right, advancing each by a float addition of scale while its integer table
index is below support. Six zero padding entries follow each side. The
returned counts are left generated count minus one and right generated count
plus one; each is rounded upward to a multiple of four by the consumer. The
source interval is `[floor(phase)-left, floor(phase)+right)`.

The earlier BSample-byte interpretation is retracted: the tested byte belongs
to the resampler context at `voice+8`, so context `+0x138` is voice `+0x140`.
Caller `0x140865ddc` forms that pointer; `0x140865de7` tests the voice byte,
and the ordinary HQ call `0x140865edf` passes the context in the first stack
argument (`0x140865eda`). Callee `0x14086ebce/ebe8` loads that argument and
tests its `+0x138`. Context initializer `0x14094d600` clears the byte at
`0x14094d603`; copy helper `0x1408312b0` preserves it. Temporary preparation
`0x140866290` sets voice `+0x140` to one at `0x14086648a`, renders through
`0x14087ab10`, saves the prepared context, restores the ordinary context,
and clears the byte at `0x1408664d7`. Its `0x14086dce0` consumer can call HQ
with the byte set; that branch computes reach directly and fetches individual
taps. The normal audible caller selects the coefficient-helper/SIMD branch.
This is a temporary producer-context flag, not sample format or residency.

The decoded convolution at `0x14086f3d0..0x14086f473` accumulates four lanes.
For each sixteen taps it adds the first four products, then the next four,
then the float sum of the last and third four-product vectors. Remaining
four-product vectors are added separately. Two horizontal additions form
`(lane0+lane1)+(lane2+lane3)` before the final scalar gain. A scalar sequential
dot product would change the evidenced arithmetic ordering.

The arbitrary-distance forward loop mapper `0x1408ed780` uses modulo for
permitted left and right taps; its left/right flags remain distinct. Current
counted quartet taps already use modulo, preserve final/negative right-side
suppression, and prepare loop lengths of at least four. Legacy virtual runs
also use modulo and the gatherer crosses as many runs as the requested stencil
needs, including one- and two-frame legacy loops. No single-length-wrap defect
was found in those current paths.

The implemented Native ordinary-sampler High/Ultra path uses the pinned
coefficient values and this convolution order for forward, unlooped samples,
including saved crops. A shared fallible kernel reads bounded chunks without a
total tap-footprint or pitch cap. RAM reads the resident source; the existing
stamped sparse worker publishes convolution outputs for streaming instead of
quartets. The packet records the source floor, existing 24-bit fraction,
actual step and resampling flag. Static near-unity points copy their center;
active pitch trajectories still convolve. Compatibility, wavetable and Normal
paths retain their own selected kernels.

Source fetching at `0x14086f2b9..0x14086f307` first applies the current physical
loop mapper, then zeros a tap outside the complete BSample frame range. This
does not establish padding at a cropped playback edge. The producer skips an
entire point once its floor reaches its terminal frame; both RAM and worker
paths implement that whole-point check before coefficient geometry. Literal
outputs captured from the native helper/SIMD branch cover negative support,
wide downsampling and right EOF. Four additional captured trajectories serve
as independent complete-PCM references for RAM, streaming and filter lanes.
The fourteen literal native boundary/wide points passed central validation.
The initial complete-PCM fixture failed its audibility assertion before PCM
comparisons: its wide periodic excitation had negligible energy in the authored
low-pass passband. The fixture now adds exact binary DC `1/32` and uses four
fresh trajectories captured by executing the same pinned native consumer;
the original fourteen landmarks, audibility assertion and PCM tolerance remain
unchanged. The revised complete-PCM fixture passed central execution (1.82 s),
as did the typed-kernel pending/publication fixture (2.68 s).

The float-distance optimization preserves repeated native float additions:
inside one binade, after tie rounding stabilizes, it advances the identical
integer mantissa stride; binade crossings perform the original additions.
It neither drops taps nor substitutes a quality when support widens. An
isolated 8385-point comparison against iterative float additions found no bit
differences. Stalled additions and unrepresentable signed source intervals
fail explicitly.

Looped, reversed and positive-crossfade HQ geometry remains checked
unsupported before PCM/source-clock admission. In particular, wide support
must use the current physical loop's left/right permissions rather than past
virtual traversal history. The current local fixed-point source clock also
does not establish full parity with native float phase grouping. Those source
domain and phase-grouping boundaries remain open.

The crop boundary is closed for the implemented physical-domain extension.
Voice initialization `0x1409691c1..0x140969239` copies zone end offset `+0x9d4`
to voice `+0x904`, clamped between `4-sampleFrames` and zero, and zone start
`+0x9d0` to voice `+0x908`, clamped between zero and `sampleFrames-4`.
Forward playback initializes physical cursor `+0x164` from that start.
Producer `0x14087ad53..0x14087ad59` supplies `sampleFrames+voice.endOffset`
as its terminal. The convolution still gates each raw tap against complete
BSample frames. Isolated native outputs with terminal 2048 and step 16.25
were bit-identical to the full-source terminal at floors 1024 and 2047 (fractions
0.25 and 0.75), for both qualities; floor 2048 instead returned zero. Thus a
crop changes whole-point termination, not filter support padding at that edge.
RAM and worker points now retain the physical start offset and stop whole
points at the saved physical terminal. Their tap readers pad only outside
the full sample header range, retaining real pre-crop and post-crop support.
Native ordinary High/Ultra bank preparation uses the full physical header
extent when deciding residency and streaming; it preserves the existing
memory quota policy. Changing prepared settings to High/Ultra on a bank that
has neither full physical residency nor streaming fails with a reload error.
Compatibility, Normal and wavetable preparation retain their previous extent.

The staged NKI/cache crop fixture uses start 1024, terminal 2048 and step
16.25, with literal native PCM for both qualities. It exercises RAM-only,
bare streaming, generous and one-byte-quota load paths, and distinguishes
incorrect zero padding at both cropped edges. It also checks the prepared
Normal-to-High/Ultra reload error. The first central crop run stopped before
playback because creating its cache directory changed the tracked library-root
mtime after import. The fixture now creates that directory before capturing
dependencies; cache validation remains intact. The corrected crop PCM check
awaits central execution.

The next zero-crossfade forward HQ loop domain is backed by complete bounded
native producer trajectories in ignored `artifacts/core-native/`: the capture
executes `0x14087ab10` through `0x140865c70`, the ordinary HQ consumer,
coefficient helper and SIMD accumulation, plus the actual row selector,
geometry helper and tap mapper. Pinned generated table values are supplied;
raw physical sample fetching and cache misses are hooked. Unused preparation
services and the zero-width crossfade helper are excluded from this source
capture. The source worklist `0x140946b90` executes the actual receiver/source
preparation `0x1409462c0` and `0x140916350` before playback. Its ordinary valid
context increments voice `+0x534` at `0x140946b51`, even with no internal source
slots. Admission enqueues such voices at `0x140906e50`, services that worklist
at `0x140906f02`, then checks voice `+0x520` readiness. An earlier direct-producer
capture omitted this mandatory worklist and left `+0x534` zero; its fragment-end
loop bypass at `0x14087aee7` is not evidence for an admitted ordinary voice.
For both qualities at exact step 16.25, count one starts with flags
zero; count two starts with right-only flag two and repeats its final pass
with left-only flag one. Count zero starts with two and repeats with three;
negative counts start with zero and repeat with one. Executing release
`0x14086aea0` sets remaining one, preserves the repeated left permission,
suppresses the right permission, skips a later Until Release row and resets
left permission on selecting the later counted row. Ordered sparse slots and
an initial partial pass at phase 400.25 are captured with literal PCM too.
This closes the physical per-point permission contract; runtime HQ loop
admission remains guarded until RAM and worker integration is complete.

The crop pipeline exposed a source serialization defect before PCM comparison:
`creator::nki::groups` previously always emitted its preserved v0x104 mode-3
source template, irrespective of authored `Group.source_mode`. Mode 3 prepares
nonzero source preload budget (the receiver consumer distinction recorded in
`MODULATION.md`), so it did not select the mode-0 Native interpolation branch.
The writer now changes only the verified source identity u32 for authored modes
0 and 3, keeps the complete surrounding private record, preserves legacy mode 3
when identity is absent, and rejects other modes before overwriting output.
The crop and prepared-quality guard assert imported mode 0; a separate raw
round trip checks both mode identities and identical opaque prefix/suffix.
Thus the earlier guard failure was an ineligible saved source, not permission
to reject an actually complete resident sample. No residency/cache guard was
weakened. These new source serialization/crop assertions await the central run.

Two additional prepared-producer captures live in the ignored native evidence
folder without replacing earlier captures. `hq-authored-offset-trajectories.json`
uses initial physical phase 390, directly representable by authored offset
500 microseconds at source rate 780000, and captures 260 sounding points for
both qualities. This supplies an actual public-API partial-pass PCM oracle;
the earlier phase400.25 capture remains useful native arithmetic evidence.
`hq-short-zero-start-trajectories.json` captures an UntilRelease count-zero
loop0..4 at step16.25. The first point has right permission only; the next
point phase0.25 has both permissions, including repeated wrapping of negative
High/Ultra support into that four-frame domain before physical negative padding.
Release after ten outputs sees pending phase18.5, consumes the final crossing
without rewinding, and emits raw physical tail18.5 (261 sounding points).
The native receiver preparation runs unchanged in both new captures; its
counter is incremented by native code, never manually seeded for admission.
No HQ loop runtime admission is enabled by these capture additions alone.

### Zero-crossfade High/Ultra loop domain integration

The prepared receiver captures now drive one shared physical-domain reader in
`hq::read`: current-row left permission is the already-looped flag; right
permission is signed remaining count >=0 and !=1. The modulo map executes
before physical negative/EOF padding, including multiple full wraps across
wide support. RAM and the typed stream worker use the same run splitting.
`hq::covered` deduplicates residency coverage of a whole repeated loop; it
never removes convolution taps. The kernel still evaluates every required
coefficient and SIMD lane. No pitch or source-footprint cap was introduced.

Each HQ point comes from a copy of the existing `CountedCursor`, advanced by
actual per-output source travel, with the final fragment crossing left pending
for release-before-normalization ordering. The same owner supplies an ended
bit: counted stream configurations intentionally cover the complete physical
asset, so their map end cannot infer a cropped voice's terminal. The point's
floor, truncated24 fraction and native-f32 step remain intact; the four-word
key carries checked current-domain bounds/permissions and ended state without
truncating an unrepresentable floor. Request/config publication stamps remain
unchanged. The renderer rejects positive crossfade, reverse, unprepared legacy
loop domains, and a remapped crop terminal disagreeing with its prepared rows
before PCM. A prepared Normal-to-HQ edit requiring new physical loop metadata
requires bank reload. Compatibility source/map preparation is unchanged.

Native-policy, ordinary mode-0 saved High/Ultra single unlimited zero-crossfade
loops prepare the same row/cursor owner as counted and ordered loops; this is
performed on the worker before bank installation. The full physical header
coverage policy and existing memory/stream decisions remain in effect.
`creator::nki::hq_loop_tests::saved_native_high_ultra_loop_pcm_matches_prepared_native_producer_in_ram_and_stream`
stages 22 literal native trajectories through actual NKI write/import/cache,
resident and real streamed Engine playback with four voices and irregular
host fragments. It includes a continuous mode-1 count-zero row whose native
release leaves remaining count zero (320 points still active), in addition to
the finite/release/ordered/negative/partial/start-zero captures above. Captured
last physical phase and sounding-point census independently apply the existing
ordinary EOF amplitude law to the native convolution output. The central compile gate and actual 22-case mode-0 complete PCM check passed
(2.54 seconds); the independent saved counted/curved source-EOF check also
passed (1.70 seconds). Native f32 producer grouping and
fractional positive-crossfade future-array ownership remain separate gaps.

## Saved DFD identity and interpolation dispatch

The saved and runtime source enums are distinct. Full BGroup construction
`0x1407f7316..1d` installs BParSrcMode's vtable `0x144e97640` at `+0xba0`;
its private reader `0x140d03aa0` reads v0x104 mode through enum converter
`0x140cf05a0`. Serialized mode0 becomes runtime0 and serialized mode3 becomes
runtime4 at `BParSrcMode+8`, hence full Group `+0xba8`. Runtime source registry
`0x140d208f0` maps both0 and4 to the identical SamplerModule singleton
`0x149e84698`, vtable `0x144fd21e8`, process `0x140d206a0`. Runtime3 instead
names TimeMachineModule; it is not the saved DFD3 identity.

Real Voice initialization `0x140968ca0` chooses null, High, or Ultra tables
from Group `+0xd8` without a source0/4 branch. Both source identities then reach
the same ordinary producer and its table-selected interpolation consumer.
DFD's nonzero BSample `+0x177c` preload budget changes readiness/prefetch:
`0x140866290` saves cursor/cache context, temporarily sets Voice `+0x140`,
runs producer `0x14087ab10`, restores that context, and clears the flag.
SamplerModule separately disables loop-control buffers when `0x140873030`
finds that budget nonzero. These conditions do not select different tables.

The bounded capture `artifacts/core-native/capture_dfd_source.py` executes
the actual enum conversion, Voice initialization, registry dispatch, and real
DFD preflight. Initialization agrees for all three qualities, retained in
`dfd-quality-initialization-partial.json`. Complete DFD PCM capture is still
incomplete: synthetic source storage lacks the native block descriptor owner
read from BSample `+0x58` at `0x14086de28`. That field was not fabricated;
partial execution is not presented as a prepared DFD audio oracle. Reader,
conversion, preload, and physical-fill captures are preserved alongside it.
At that partial-capture checkpoint, saved mode3 still kept the prior Hermite
interpolation for qualities0/1/2. The subsequent prepared capture below
supersedes the High/Ultra capture limitation.

A subsequent prepared capture closes that descriptor gap for High/Ultra.
Actual `0x14082a680` partitions the saved physical asset into preload/block
ranges, allocates 72-byte descriptors, writes their physical block start and
length at `+0x38/+0x3c`, and installs the array at BSample `+0x58` with its
count at `+0x50`. `capture_dfd_prepared.py` executes this constructor with an
explicit 4096-byte positive budget and one initially preloaded 512-frame block;
CRT allocation/qsort and deterministic physical sample supply are isolated
harness services. The budget is a capture input, not a proved loader default.
The actual DFD preflight and SamplerModule process then complete without
stubbing cursor save/prefetch/restore. `dfd-hq-sampler-comparison.json` retains
256 outputs per quality: all 253 sounding points and three EOF points are
bit-identical between saved0 and saved3 at source step16.25.

`dfd-loop-comparison.json` records fourteen further native comparisons, High
and Ultra for count1/count2, negative/i32MIN/zero UntilRelease with later rows,
ordered sparse rows, and partial offset400.25. All 320 PCM points and every
captured per-point loop domain/permission match between the two saved modes;
release uses the actual native release method. Native DFD preflight executes
twice per ordinary source call and preserves the delivered cursor trajectory.
These captures establish the prepared High/Ultra interpolation and forward
zero-crossfade loop laws for this backend adapter. They do not reproduce NI's
asynchronous disk scheduling. Quality0 still stops in the synthetic fast-cache
path at `0x14086e5c9`; no new quality0 audio capture is claimed. The following source integration consumes this bounded High/Ultra law.

Native-policy saved DFD3 now uses the existing High/Ultra kernel, physical
read coverage, and zero-crossfade row/cursor domains. Worker preparation
decodes the authoritative private controller-options record once into its
existing typed options/policy pair before source-row preparation. Compatibility
DFD and quality0 retain their previous behavior; the quality0 native fast-cache
capture remains open. Reverse, alternating, tuned and positive-crossfade HQ
geometry retain their existing admission boundaries. Prepared HQ edits still
require physical support and prepared domains; immutable source/quality changes
still replace bank ownership. The existing actual-NKI/cache/complete stereo
PCM fixture now runs all22 scenarios for both saved0 and saved3, preserving
native literals, resident/real-stream comparisons, offsets, exact terminals,
allocation and underrun assertions. The central 44-case complete PCM check
passed (5.04 seconds); no NI asynchronous I/O parity claim is made.

## DFD Normal interpolation and physical crop support

The quality0 capture gap is now closed in the isolated ready-source adapter.
Actual SRC reset `0x14094d600` initializes cache indices to−1. Buffer-pool
preparation `0x1408edae0`/`0x1407cbea0` allocates 2112-byte source buffers;
actual Voice source preparation `0x14096a380..3f7` assigns Synth/Voice ownership
and binds buffers selected by `0x14081ebc0`. The capture executes this bounded
source-preparation prefix before later unrelated group-allocation work. It
does not fabricate the context owners, cache indices or buffer pointers.
The selected16-slot pool and4096-byte budget are explicit capture inputs.
`0x140828500` executes native format0 float32 block filling, actual DFD
descriptor construction/preflight and the table-null ordinary producer execute.
Ready physical bytes and logged asynchronous receipt submission are adapter
services; NI disk deadline/scheduling behavior is not reproduced.

`dfd-normal-trajectories.json` records256 bit-identical saved0/3 outputs.
`dfd-normal-loop-comparison.json` records seven320-point paired comparisons
(count1/count2, negative/i32MIN/zero UntilRelease, sparse ordered rows and
partial offset400.25); every PCM bit and captured point domain agrees.
`dfd-normal-crop-comparison.json` also retains explicit physical boundaries:
phase1024.25 before cropped end2048 gives `0x3d501000`, including physical
frame1023; phase2047.75 gives `0x3d5b3800`, including physical2048/2049;
floor2048 stops the whole point. At actual asset EOF4096, phase4095.75 gives
`0xbcd70000`, proving zero right padding (repeat-last gives `0xbdb66000`);
floor4096 stops the whole point. A separate phase0.25 point gives
`0xbdf17c00`, proving zero left padding rather than repeating frame0.
These exact quarter-phase Lagrange weights and binary source values independently
distinguish physical support from crop clipping.

Current Normal gathering for an unlooped crop instead pads relative negative
taps and stops support at the crop end. That is a concrete source0 geometry
gap as well as a blocker for a coherent source3 Normal extension. Native
quality0 eligibility remains unchanged until physical-window preparation and
RAM/worker ownership are integrated; the captured DFD Normal law is not yet
claimed as implemented playback.

### Prepared DFD Normal and physical crop quartet (2026-10-04)

The protected `artifacts/core-native/capture_dfd_normal_fixture.py` executes
actual saved-mode conversion, Group quality initialization, sample descriptor
preparation, source-context/pool binding, native DFD preflight and the ordinary
SamplerModule producer. Eleven complete 320-point Normal trajectories agree
bit-for-bit for saved source0 and saved DFD3: counted first/final passes,
negative/i32-min/zero UntilRelease, sparse ordered rows, release immediately
before/after a boundary, authored offset, a short zero-start loop and continuous
looping. The physical source provider supplies ready float blocks and records
cache receipt submission. This establishes the selected kernel/domain law; NI
asynchronous cache deadlines remain outside this isolated capture.

`capture_dfd_normal_crop_fixture.py` independently captures seven paired
trajectories with actual source rates: crop1024..2048 at quarter-frame phase,
full physical EOF, assets1/2/3, two-frame source cropped to one, and a one-frame
source at a 1MHz host clock. Normal reads the physical quartet outside the
playback crop, zeroes only outside the complete source, and stops the entire
point at the crop terminal. Examples: phase1024.25 yields `0x3d501000` using
physical1023; phase2047.75 yields `0x3d5b3800` using physical2048/2049; floor2048
is silent. An asset2 cropped to1 still produces `0xbdf01400` at phase0.25;
asset1 produces `0xbdd20000`. These are native output bits, not Rust-derived
interpolation expectations.

The implementation reuses empty prepared Rows and the existing physical sparse
quartet service for forward Normal crops, including tiny assets. Active saved
loops still require native minimum4 and signed frame bounds. Empty Rows have an
exact linear remaining-source distance and require no loop lookahead horizon;
active-loop preview admission is unchanged. Normal worker preparation retains
two physical frames beyond the crop (capped at the full header), under the
existing memory/residency policy. Prepared settings validate the actual required
physical support. DFD Normal uses the selected Lagrange path for the captured
forward zero-crossfade geometry. Retained authored DFD reverse/alternating/
positive-crossfade state is diagnosed before normalization can erase flags;
live effective unsupported geometry fails source preflight. Compatibility,
wavetable and existing source0 directions/crossfades are unchanged.

The existing NKI/cache/RAM/real-stream complete-PCM fixture now includes these
18 literal Normal captures for both saved identities, alongside the22 High/Ultra
captures. It keeps exact endpoint census, irregular fragmentation, no render
heap allocation, zero underruns and the independent ordinary EOF/onset gain
reference. This extension is staged for central execution; no test was run by
this implementation owner.

### Capture loop-owner correction (2026-10-04)

The first DFD loop-comparison and new Normal-loop fixture captures populated the
standalone harness LoopArray after the real Voice initializer had rebound
`Voice+0x560` to `Zone+0x7d8`. Those inputs were unconsumed: index8/remaining0
showed plain playback. Their equality establishes the plain prepared kernel
bridge only, not counted/ordered loop domains. The earlier DFD loop-domain
claims above are superseded; original JSON is retained as
`*-unbound-loop-input.json` for inspection. The first central Normal count2 PCM
failure (maximum error0.71875) exposed this capture defect; its source-byte
recipe was correct, and no production interpolation equation changed.

`capture_dfd_source.py` now reads the actual LoopArray pointer produced by
`968ca0` before saved rows are populated and `960c00` selects them. The recaptured
Normal fixture executes actual bound rows in both saved0/3: real index/count
selection, repeat transitions and release skipping, with identical complete
output. Only the incorrect eleven Normal loop literal arrays are replaced;
previous HQ reference literals and the seven empty/crop/tiny captures are
unchanged. The High/Ultra DFD comparison is likewise recaptured against bound
Zone storage rather than the stale harness array. The ordinary ready-block
adapter remains an explicit asynchronous-deadline boundary.

The complete NKI fixture now places actual WAVs under its private `Samples`
library root and cache files in a separate temporary root. This bounds import
root dependencies and keeps unrelated `/tmp` directory activity from
invalidating the cache proof; dependency validation is unchanged.

### Normal single UntilRelease pending overshoot (2026-10-04)

The corrected Normal native fixture exposed a production ownership defect in
saved0's legacy single-unlimited UntilRelease map. At step16.25, loop0..4 and
release before output10, the real bound producer retains physical phase18.5
(the final emitted phase2.25 plus its pending step). Release sets remaining1;
selection then exits the current loop without subtracting its length. Physical
EOF therefore owns output261. The virtual-map path had already reduced that
pending phase to2.5 and extended the tail by one output. This is an actual phase
trajectory difference, not an oracle endpoint adjustment.

Kontakt Normal forward zero-crossfade single UntilRelease rows now use the
same prepared physical cursor as counted/HQ/DFD rows. The saved loop metadata,
source clock, interpolation and gain equations are unchanged. Continuous
saved0's ordinary fast path, Compatibility, reverse/alternating and crossfade
paths remain unchanged. The existing literal-native short-zero-start case keeps
its261 endpoint and complete PCM reference; central execution follows the
source freeze.

### Public crop fixture start receipt correction (2026-10-04)

The public NoteEvent offset receiver in `Player::spawn` converts microseconds to
integer source frames before admission. At125kHz,2us therefore admits offset0,
not0.25. The first crop fixture incorrectly paired that request with a direct
producer phase1024.25 and4095 points, then applied a nonzero-offset onset ramp.
Its central endpoint discrepancy (4095 still active) exposed this fixture
receipt mismatch. Production cursor/terminal/padding policy is unchanged.

The actual NKI crop fixture now authors offset0 and captures the native producer
from physical1024, with4096 sounding points. Output1 is1024.25 and still proves
physical1023 support before the crop; output4095 is2047.75 and still proves
physical2048/2049 support after it. Output4096 is silent at the same exclusive
terminal2048. The earlier quarter-origin producer capture remains protected as
`dfd-normal-crop-fixture-quarter-origin.json`, useful for isolated kernel input
but not a public offset receipt claim. Other Normal literal cases and tolerance
are unchanged; the authored500us/390-frame offset cases still exercise the
existing public offset/onset policy.

### Required Normal physical domain cannot retain a legacy fallback

Kontakt Normal forward zero-crossfade UntilRelease must have prepared physical
Rows; DFD Normal requires that domain for forward zero-crossfade loops too.
`Voice::prepare_source` now rejects a retained legacy loop in those scopes with
`UnsupportedWindow`, before source-clock/PCM commit. This covers both failed
preparation and absent retained rows, without another mutable zone/play flag.
It leaves Compatibility and saved0 Normal Continuous's verified fast path
unchanged. A short authored length is not automatically unsupported: native
normalization raises it to4 when the physical asset/crop can contain it.

The focused actual-NKI/cache source-admission fixture stages two counterexamples
for saved0/3, resident and real-streamed: a3-frame asset with active mode2/count0,
and asset100/crop12/loop10+length2 (normalizes to14 beyond crop). It requires the
explicit source failure, complete unchanged PCM/lifetime for a previously
sounding sibling and zero render allocation/underruns. A valid4-frame prepared
loop and an explicit Compatibility3-frame loop remain audible. These checks
are staged for central execution, not run by this owner.


## Saved interpolation quality admission is exactly three values

The protected bounded disassembly is
`artifacts/core-native/native-saved-interpolation-quality-admission.asm`.
It records PE image base `0x140000000` and SHA256
`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`
for the pinned installed Kontakt 8 executable. This confirms, rather than
extends, the saved field provenance above: public Group reader
`0x140d0cf80` accepts versions `0x90..0x96`; its three version branches call
`0x140cf0360` with receiver `+0xc8` (full BGroup `+0xd8`).

The enum reader calls the signed integer reader at `0x140cf0369`, stores
zero at `0x140cf0394`, one at `0x140cf0388`, or two at `0x140cf037c`.
Every other original integer takes `0x140cf037a` to `0x140cf03a0`, constructs
an exception, and reaches the throw at `0x140cf03b6`. Thus an unknown saved
quality is rejected by the native archive reader; it does not select a fourth
ordinary sampler kernel. XML reader `0x140920cd2` independently recognizes
only `normal`, `high`, and `ultra`, mapping them to 0/1/2.

The separate global quality override compares against three at
`0x140968ea6`; three reads the saved Group field at `0x140968eab` (inherit).
It is not a valid fourth saved value. The ordinary Group-only initializer
at `0x140968ee0` selects null tables for zero and the two table pairs for
one/two; that internal initializer's null-table handling for other integers
does not bypass the preceding archive rejection.

KONTRA's retained archive integer and effective prepared quality need the
same capability boundary for Kontakt ordinary source identities 0/3.
Otherwise an out-of-range integer declines both Native kernel predicates
and reaches the Compatibility cubic without a diagnostic. The production
owner is integrating this into existing prepared source admission and the
`with_prepared_settings` effective-quality check; raw metadata remains
preserved, Compatibility behavior remains unchanged. This evidence does not
establish an external offline/global-override control API.


## Sampler-only alternating rows that cannot reflect

At `0x14087b112` a positive count is decremented before deciding whether to
turn. Count one takes the exhausted path at `0x14087b11c`, selecting the next
row at `0x14087b16a` before the alternating branch at `0x14087b180` is reached.
On the initial pass both tap-wrap permissions are false. A unity-tuned,
zero-crossfade alternating row with saved count one therefore never changes
direction when admitted from the forward Sampler trajectory.

Protected `capture_dfd_alternating_single_pass.py` and
`dfd-alternating-count-one-trajectories.json` record nine completed **saved
Sampler0** pairs: Normal/High/Ultra at steps 0.5, 1, and 16.25, followed by a
forward count-two row. Full 128-point PCM and final receiver states equal the
same inputs with only the alternating marker cleared. The corresponding
`capture_dfd_alternating_piecewise.py` and
`sampler-alternating-count-one-piecewise.json` record twelve more completed
pairs, varying consumer steps 0.5/1.25/16.25/2/0.25, initial positions 0 and
5.25, and early/late UntilRelease inputs. Every PCM point and per-call
cursor/count/direction state agrees. These scripts use the actual initialized
loop array and bound source context; no mirrored cursor or count field is
forged after selection.

`Native.call` now requires a real STOP return receipt, with the separately
recorded `0x14096a380` binding prefix stop at `0x14096a3f7` explicitly allowed.
An earlier exploratory DFD wide-step attempt produced partial zeros after its
instruction budget, which is **not** a completed native PCM counterexample.
The unbound High unit attempt also lacked the raw context used by the existing
prepared captures. Neither attempt authorizes DFD alternating admission.
Completed DFD, reflective alternating, and fractional producer grouping remain
separate requirements. The single-source proof here does not assert arbitrary
Native fragment invariance for reflecting loops.

Prepared Rows receive an explicit `nonreflecting_single_pass` capability only
from the shared Bank predicate for Kontakt saved Sampler0, ordinary source,
quality 0/1/2. It admits an alternating marker only for count one and raw zero
crossfade; all other alternating rows retain their existing diagnosis. DFD and
Compatibility remain unchanged. The raw marker stays in Zone/archive/cache;
the existing forward cursor expresses this non-reflecting execution without a
new direction clock or renderer path. Both initial and residency preparation
use the same capability predicate.

The staged actual NKI/cache proof
`saved_sampler_nonreflecting_alternating_count_one_keeps_later_row_pcm_in_ram_and_stream`
compares the marker-bearing source against the independently validated
flag-cleared forward path, through RAM and actual streaming, full stereo PCM,
partial saved starts, early/late release, live retuning, irregular fragments,
exact fixed-step EOF census and zero render heap allocations. The later
count-two row makes the prior finite fallback audibly different; no test has
been executed by the implementation agent.
