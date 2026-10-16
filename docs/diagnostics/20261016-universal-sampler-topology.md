# Universal sampler topology: implementation contract

This is an implementation contract, not completed playback support. The goal is
one compiled signal and lifetime model whose format adapters describe their
actual behavior. It must not impose UVI's processing instances on Kontakt or
retain the current stereo-only source boundary as a universal law.

## Ownership that must remain distinct

| Identity | Meaning | Must not imply |
| --- | --- | --- |
| Note | Physical or generated event, expression and release relationship | One oscillator or one effect instance |
| Selection family | One coordinated selection/take decision | Keygroup-wide nonlinear history |
| Processing instance | One attack's activation of an authored processing scope | A global bus shared by every note with the same keygroup |
| Source instance | Oscillator invocation, immutable source revision and independent cursor | One instance per stereo storage pack |
| Storage identity | Immutable channel payload or page dependency | Shared phase, envelope, effect history or note lifetime |

An immutable definition, the state-instantiation rule and the destination binding
are also separate. A modulator declared on a program can require per-attack
state when targeting a lower voice control. Parameter names and declaration
ancestry alone cannot choose its state owner.

Current source demonstrates the mismatch. `Family` coordinates admission and
selection; `Voice` contains one sample, cursor, envelope and chain. DSP state is
claimed by physical voice slot. Several regions referencing the same immutable
chain therefore receive separate histories. A global keygroup bus would instead
share histories between independent attacks. Neither expresses the required
middle scope.

## Format adapters and shared execution

Kontakt's serialized group insert rack currently becomes a per-zone
`Scope::Voice` chain, with its saved amplifier split and send positions. Preserve
that law. Instrument insert, send-return and main processing remain summed
bus instances. A saved group is a parameter/selection namespace, not sufficient
authority to share nonlinear state.

For UVI, the documented hierarchy places oscillator sources inside a keygroup
voice and effects above those sources. The adapter must retain selected and
bypassed oscillator identities, individual source clocks, ordered shared stages,
and per-attack processing state. Modern documentation is a useful contract;
exact Workstation stage and control joins still need byte-level evidence.

Compile explicit signal operations: source reads, source-local stages, ordered
sums, matrices, shared processor sections, amplifiers, taps and bus emission.
Signal edges carry channel layout and arithmetic law independently of their
physical storage. One six-channel source has one interpolation phase and loop;
three detuned oscillators have three independent phases. Preserve channels until
an actual graph operation changes their layout. Decode-time matrix baking or
first-two-channel truncation cannot implement live matrices and bypass.

Existing singleton stereo graphs should compile to the existing specialized
typed schedule. Do not rewrite Kontakt kernels, change their rounding/control
cadence, or replace typed prepared owners with repeated loose enum projections.
New shared scopes initially use a bounded scalar schedule; parallel and batch
dispatch must schedule whole processing instances without concurrent claims of
the same mutable history.

## Lifetime and controls

Reserve processing state and selected member sources before publishing their
links. Preserve the format's existing selection-consumption law; do not assume
that a failed attack must rewind every native random or round-robin decision.
All channel dependencies of one source must become ready together.

Source completion and processing-instance completion are different events. A
shared release envelope may need continuing source audio after note-off. Shared
tails can outlive every source, retaining their original generation without a
dummy sample voice. Release, choke, stealing, incoming plan publication and
retirement must account for both owners. A script fault must not prevent
terminal release of the original note.

Compile source, processing-instance and global/bus control targets explicitly.
Immediate engine writes, queued controls, offsets, recall and snapshot overlay
must resolve through the same typed target/domain validation. Retain bypassed
members and processors; an observed bypass is not permission to erase topology.

Account for source payload, decoded scratch, channel pages/heads, codec readers,
file handles, processor state, and overlapping active/incoming/retired owners.
A per-reader estimate or a per-part quota is not an aggregate runtime bound.

## First executable slice and acceptance

The first code slice must render two independently clocked ordinary resident
sources through one explicitly shared processor section using normal
IR/lower/Runtime entrypoints. Two overlapping attacks must retain independent
histories. This is positive execution, not another ExecutionPending prototype.

Signed cancellation before a rectifier distinguishes processing the source sum
from summing separately processed sources. Separate-keygroup and staggered
attack controls rule out a globally shared shortcut. A stateful processor,
nonzero release waveform, source exhaustion, old-generation tails and late
transaction refusal cover the ownership law beyond a memoryless example.
These authored tests establish the graph contract, not native UVI mode numbering
or actual instrument timbre.

The first executable slice is implemented and validated at code commit
`634cb6788a8ac295b9696ab0cf245ca98bcf8790`. Eighteen authored topology tests pass,
including native Kontakt amplitude preservation/refusal controls. The complete
core/IR library and integration test command, affected all-target Clippy,
Kontakt/UVI library tests, and standalone all-target check pass. The same-policy
Doctor producer completed with no producer errors, zero blocking diagnostics,
and 11,619 warnings; these warnings remain visible and no rules were waived.

The broad test run exposed an existing envelope-schema allocation failure:
the unchanged baseline requested 536,140 bytes against its 524,288-byte limit.
Reserving known batch dimensions reduced the request to 290,860 bytes. The test
and limit remain unchanged. Baseline and corrected receipts are retained.

This slice has explicit resident stereo edges, scalar shared processing and an
opt-in IR definition. Existing format adapters do not yet emit the new scopes.
No six-channel playback, full streaming cohort, actual VWinds fix, binary
installation or shipping acceptance is established by these passing tests.

Next, the normal UVI adapter and full-channel source lifecycle must emit and
execute the actual Clarinet topology, including all three retained players,
matrix, OnePole, envelope placement, sends and live control paths. Native DSP
equations remain attached to their own evidence identities. Matching generic
tests does not establish the real instrument's output.

## Static reference checkpoint

Read-only mapped RTTI discovery over the retained Workstation analysis image
`c997d9ce5ca6619a74d159a3424003cb5f76f7c51825f2234b2d057ee1ac9925`
found 169 relevant type candidates and 198 validated locator/vtable-pointer
relationships. Pointer runs remain candidate extents, not method names or
runtime ownership proof. No activation, loader, target or bank code ran.

UNPACER's static function frontier and REA/Ghidra 12.1.4 admitted 13 targeted
bodies totaling 17,843 bytes, with zero synthetic bytes in the admitted ranges.
The evidence index distinguishes actual analyzed body ranges from unwind
metadata spans. In particular, the property declaration has a 5,630-byte unwind
closure but only 5,613 admitted body bytes in separated ranges. Byte equality
does not by itself prove decompiler semantics or whole-application coverage.

The keygroup processing body at `0x140f28530` selects retained oscillator
members, renders them, combines their outputs, then runs the ordered insert
wrapper. Layout binding at `0x140f26230` considers every retained child, including
bypassed children: one child can bind directly to the output, while multiple
children use scratch with the selected layout. Sharing that scratch does not
mean sharing their source clocks. The oscillator wrapper at `0x140ece100` has a
32-frame subdivision branch and a distinct full-slice branch; the mask's
upstream state law is still unresolved and must not be replaced by the current
core's 64-frame cadence without an explicit format profile.

The exact `FXPostGain` literal-to-property registration, constructor binding,
getter/setter and processing branch are joined. Its native storage is bit `0x02`
at keygroup offset `0x410`: true applies the gain-like stage before ordered
inserts, false after them. The constructor initializes false before restoration.
The stage's control handle, smoother and other state factors remain separately
named facts; this does not yet establish every factor's public parameter name
or envelope provenance. The other insert-bypass branch is not assigned a public
name without its own registration join.

The combine dispatcher reaches a distinct layout conversion helper at
`0x141353410`. That helper uses an input-major native conversion matrix and
normalization, then adds contributions in order. This operation is separate from
the programmable GainMatrix insert. Four bounded static selector paths were
reconstructed, with every instruction and table read hashed: descriptor `6→6`
is identity; `6→2` maps inputs 1 and 5 to the first output, 2 and 6 to the second,
drops 3 and 4, and scales by float32 `0.5773502588272095`; `2→6` retains the first
two outputs; `1→6` targets the third output. These are descriptor-code paths,
not permission to identify every six-channel file with descriptor 6. Actual
negotiated layout and downstream projection still need their callsite joins.
The `6→2` and `2→6` coefficient arrays are in writable `.data`: these are exact
on-disk initial values, not yet proved immutable runtime coefficients. Their
initialization/write closure remains open. The small static interpreter is
admitted only for these four reviewed paths, not arbitrary x86 condition flow.

This is targeted DSP analysis, not whole-application decompilation or runtime
restoration. No target, activation or bank code ran for this static checkpoint.

The immutable M2 packet remains unapplied. It rejects the actual multi-player
shape and does not substitute for this implementation. No new binary has been
installed and no universal parity claim is made.

Source audits, candidate interfaces, exact static bundles and test plans are
retained under `PluginArtifacts/kontra-universal-topology-20261016-r18/`.

References: [UVI structure](https://manual.uvi.net/falcon/en/structure/),
[Keygroup API](https://lua.uvi.net/class_keygroup.html),
[Oscillator API](https://lua.uvi.net/class_oscillator.html),
the [existing parity contract](20261016-vwinds-parity-contract.md), and
[Doctor ownership evidence](20261016-doctor-remediation.md).
