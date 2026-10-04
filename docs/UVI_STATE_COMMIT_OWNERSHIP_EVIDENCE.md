# Final saved-state endpoint admission

The controller's final state commit previously checked matching inspection
context and View identity, but did not recheck installed audio-endpoint ownership.
Those inspection checks deliberately remain valid for some failed or unadopted
contexts. If the endpoint failed or changed after the earlier caller check,
captured bytes could replace the rack state and saved baseline despite that
loss of ownership. This is a source-supported gap, not an observed attribution
of the user's playback failure.

The correction reuses the existing installed-endpoint predicate: matching UVI
generation, matching part generation and no endpoint failure. The final commit
checks it after source-context validity and before any target is written. Every
target must pass before either rack bytes or saved baselines change. Existing
`installed` callers retain their lazy context-then-endpoint order.

The View then Selection lock order is unchanged. Context matching uses the
already-held Selection guard; it does not recursively call `current` or
`installed`. Endpoint lookup briefly uses the existing Shared.parts lock and
retains an Arc, following the order already used by context matching.

Independent static review checked the source and authored failure/replacement/
unadopted rejection, whole-batch foreign-source rejection and successful-save
expectations. Tests remain uncompiled and unrun under the CPU restriction.
This is final admission at sampled atomics; it cannot guarantee that audio does
not fail immediately afterward. MPE, parent ownership, runtime persistence and
installed binaries are unchanged. Combined verification remains pending.

## Rack file save and frame merging

A second source-supported interleaving involved the UI's rack Save As. It
captured native state directly into the frame's local Selection. If a later
host save published newer bytes before the UI frame merged, those earlier
captured bytes looked like a deliberate UI edit and could replace the newer
live bytes while the activation baseline remained newer.

Rack file save now captures into a local Selection copy when UVI is present.
The file owns that accepted snapshot. The frame's native bytes stay unchanged,
so the existing merge adopts a newer controller publication while preserving
the independent multi-path edit. Save-form and file-picker callers, non-UVI
and feature-disabled behavior, file-error ordering and rescan semantics remain
unchanged. The final sampled endpoint check is still required and unchanged.

Independent review traced the capture, file serialization and production merge
interleaving. A private authored source harness is prepared but unrun; its capture
and host-publication facades do not establish real plugin/GUI persistence.
Compilation, actual save interleavings and final runtime verification remain
pending. This change does not protect unrelated deliberate native-state edits
or promise an atomic save against subsequent audio failures.
