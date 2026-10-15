# Same-owner Kontakt preset application

The product loader can submit a compatible snapshot to the existing owner's
control queue. It does not prepare or install another Runtime, rerun `on init`,
replace ScriptViews, reset the interface generation, or replace the resource
resolver, tree, mapping index or decoded artwork. The ordinary UI service
publishes callback effects as patches. This is an initial represented subset,
not universal native snapshot parity.

## Admission

- The exact setup/source slot identities, sample rate and effective KSP snapshot
  policy must match the prepared owner. Modes 0 and 2 require initialization and
  continue through the full loader. Modes 1 and 3 can qualify.
- Source/resource authority is captured before initialization and rechecked
  afterward and during incoming preparation. It includes the complete discovered
  namespace and file stat identities (Unix inode/change time included), not a
  cryptographic byte identity. Partial bounded discovery disables reuse. A fresh
  namespace scan detects newly added loose resources.
- Incoming binding and saved rows decode from one immutable chunk read, avoiding
  validation of one snapshot followed by rows from a replaced path.
- This first adapter rejects incoming native group/effect payloads. It does not
  assume equal slot numbers, equal graph sizes, or unchanged code implies every
  saved native state has been represented. It does not clear translation gaps.
- Only exact Snapshot-persistent rows are mapped, across physical slots 0..4.
  All six Saved variants retain their declared shape. Missing, duplicate,
  unrecognized, out-of-domain or over-limit data refuses the entire batch.
  Ordinary and instrument-persistent values are retained.
- Persisted menus currently refuse with `MenuRosterUnverified`: saved item
  positions cannot be treated as runtime item values. An atomic live menu roster
  contract is required before enabling this case.

The FIFO-linearized operation stamps the exact current value fence at its audio
queue boundary, retaining earlier admitted gestures before incoming snapshot
homes replace their values. The product supplies owner generation and request
sequence atomics. Full preflight checks the active plan, epochs, domains,
capacities, outstanding events/commands/services/startup, all live generations' DSP tails, and callback safety. Product admission additionally checks pending host-alignment ownership and filter history only when an actual snapshot operation is polled. Cold source/resource preparation runs on a dedicated worker while the existing owner's UI/control service continues on the caller every 20 ms.
Frontend source elisions and warnings taint the exact entry and its reachable functions; a NOP left by an unsupported command cannot become a fabricated safe callback. Denial is monotone through Program builders and is recomputed when an initializer cache relowers source. Proved parked compiler listener drivers can stay parked. Arbitrary suspended
callbacks and unsafe callback graphs cannot qualify. Rejection does not run
scheduled controls or partially mutate state.

At most 8192 script/native scalar homes commit, with bounded text storage and
five callbacks. The product admits at most two owned payloads per part while
coalescing further gestures into one lightweight latest request. An inactive
host cannot accumulate one large saved-state payload per gesture. All payloads
return in replies and retire off audio; no allocation or destruction is added to
the audio operation.

## Publication and fallback

An admitted reply and callback completion are separate facts. Waiting/faulting
callbacks remain normal runtime outcomes, not successful completion or a
fictional rollback. The producer updates selected-preset metadata only for the
same base/owner/host-recall revision. Updating the receipt for a committed A does
not invalidate a newer B request for that same base. A rejected latest request
can prepare privately through the ordinary loader with reuse disabled; an
unacknowledged command is never guessed to have failed and replayed.

Host recall invalidates pending request epochs immediately. A real owner
replacement retires its old ingress and replies off audio. A successful same-owner
apply updates `PartView::attempted` so the following loader heartbeat does not
mistake the new preset label for a request to rebuild it.

Selection, request and view locks retain the editor's selection-before-view
order. Request sequence `u64::MAX` is reserved as an exhausted cancellation
sentinel, so host recall can invalidate the last admissible request without
wrapping or accidentally preserving a queued operation.

## Inspector

`corpus-health preset-switch PLAN.json` accepts an explicit action:

```json
{"op":"snapshot","path":"/absolute/owned/Preset.nksn","release_held":true}
```

Use a preceding `load` action for the base. `release_held` explicitly sends the
probe's held-note release and renders 4096 frames before admission. It does not
blanket-stop native events or conceal a busy-owner refusal. The action calls the
same prepared mapper and queued native operation, never `Core::install`; refusal
is reported without silently changing to a full load. Fresh-note PCM, bounded
callback progress, state identities and optional UI projection remain separate
observations. The diagnostic host's epoch assignment is not evidence of plugin
publication/host-recall concurrency; authored product tests cover those races.

Evidence belongs to the exact source/test cohort under PluginArtifacts. Do not
infer universal library coverage, held-voice continuity, native parity, or a CPU
percentage from source-free transaction tests.

Initial-source authority capture and revalidation have explicit `snapshot_reuse_seed`, `snapshot_reuse_authority` and `snapshot_reuse_prepare` load spans. Fresh namespace scans add preparation work; these spans must be measured before claiming an end-to-end loading speedup.
