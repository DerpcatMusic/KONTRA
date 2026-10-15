# Preset reload ownership and diagnostics

The installed build remains `5044359961c1`. The changes below are branch work,
not an installed update or a claim of complete library compatibility.

## Artwork ownership

Kontakt preset preparation now captures a resource revision on the loader:
resolved base path, program, resource root, the complete discovered file list,
file sizes and modification times, and Unix device/inode/change times. Unknown
or missing identities disable reuse. No metadata reads run in the editor.

The editor retains its existing bounded artwork worker/cache across a new audio
generation only when those revisions match. Asset path/kind/frame/size keys
still control individual cache entries. Values, gestures, drafts and native UI
script state are reset for the new generation. Equivalent pages/bindings may
retain navigation. A changed base, resource, program or asset descriptor cannot
silently inherit unrelated artwork. This currently covers Kontakt resources;
UVI does not yet supply the required namespace revision.

This avoids restarting artwork discovery and decoding for unchanged resources.
It does **not** implement snapshot hot application or eliminate script/model
initialization. The current product path prepares and installs a complete new
runtime; held-voice transition and native snapshot-mode rules remain separate
work. A warm-cache speed percentage has not been measured.

## Admission evidence

The cumulative capacity-drop counter includes several runtime admission paths,
so it is labelled `RuntimeAdmission`, rather than claiming every event exhausts
the voice pool. New branch-local counters preserve confirmed voice-slot
refusals and silent waiting-voice replacements separately. The existing total
is retained; those observations must not be added to it as independent errors.
The shell's atomic handoff preserves both fields without heap work.

Rejected UI replies now record operation kind, plan identity, expected revision,
edit count, first widget identity/index and behavior identity. Control values,
text and paths are excluded. This supplies context for future occurrences;
the six older `Capacity` replies cannot be retroactively classified.
Widget identities use exact `0x`-prefixed 32-digit hexadecimal strings. JSON
numbers cannot represent all authored 128-bit identities; the maximum-width
regression prevents diagnostic formatting from panicking on rejected edits.

Failure replies now also carry bounded callback-arena evidence captured at the
actual failed admission: capacity, reservations, queue/outbox occupancy, a scan
of at most 4,096 occupied callbacks and at most eight numeric owner samples.
Truncation is explicit. The audio-side capture allocates nothing; JSON formatting
runs after handoff. Pure input validation precedes capacity preflight, so a full
arena no longer masks an independently invalid type, range or revision.

An authored normal-limit test disproved autonomous timer accumulation: one
blocked listener and its driver retain two callbacks. Another 127 independently
admitted UI callbacks can fill the 129-slot arena during an effect-drain gap.
Eight bounded drains recover that fixture; servicing 16 effects per block across
1,000 gestures avoids rejection. This establishes a general backpressure case,
not the occupants behind the six historical failures.

## Host service during preparation

The serialized loader previously serviced committed script effects and control
replies once before waiting for private preparation. Long preparation therefore
delayed the next background service pass. Preparation already has a dedicated
worker; its waiting caller now services those existing owners on a 20 ms polling
schedule. Continuous progress messages also check the deadline. Cancellation
continues servicing until the worker closes its channel and is joined.

One effect pass consumes only its initial queue length, preserving FIFO order
while leaving new producer work for the next pass. Publication still checks the
committed generation. No candidate is installed by this service routine. UVI
publication retains its existing path and lock policy. The polling period is
not a latency guarantee: existing effect/model work may take longer.

This keeps the old owner moving during preparation; it does not preserve its
runtime across a successful full replacement. Snapshot mode 1 still requires a
complete setup-state contract, including private/UI/PGS state and safe event
identity handling. Skipping initialization or restoring persistent controls
alone is insufficient.

## Verification

The integrated root library suite subsequently passed 711 tests with 34 ignored
and zero failures. Its retained-owner fixture uses real queued controls, callback
effects and held PCM while a worker cannot finish until service advances. The
suite also checks failed/canceled replacement and stale-owner commands. Exact
source, binary and log hashes are recorded privately in
`PluginArtifacts/kontra-diagnostic-fixes/load-service-integration-receipt-20261015.json`.
This is root-library coverage, not a new installed release or complete corpus
and native parity validation.

The frozen CI test executable SHA-256
`3b32b2312637a718bcebffb1b59c92a6eb1927c0814cadb446fac241aa9a7fde`
passed 23 focused tests: resource identity (1), rejection privacy/context (1),
authored face/projection/publication (14), atomic report handoff (1), and grouped
diagnostics (6). The cache test retains the exact artwork worker identity while
clearing old generation state; source replacement/missing resource/program
tests reject reuse. Existing face tests check generation-specific interaction
surfaces and raw/resolved publication behavior.

Private logs and the machine-readable test receipt are under
`PluginArtifacts/kontra-diagnostic-fixes/preset-resource-cache-focused-tests.*`.
No plugin build, installation, DAW restart, all-library audio run or native
equivalence claim follows from these unit checks. See the
[ranked library triage](../diagnostics/20261015-library-triage.md) for the broader
failure inventory and its coverage boundaries.
