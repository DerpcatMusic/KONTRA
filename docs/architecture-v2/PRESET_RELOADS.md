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

## Verification

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
