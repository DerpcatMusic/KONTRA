# UI assets across preset changes

The UI resource owner and the current presentation have separate lifetimes. A
compatible same-owner Kontakt snapshot keeps the Runtime/PlanId, UI generation,
ScriptViews, resource resolver and Face. Normal callback publications update the
current interface. This is a conservative admitted path, not permission to reuse
every native snapshot payload; the admission limits remain in
[SNAPSHOT_REUSE.md](SNAPSHOT_REUSE.md).

## Identity and invalidation

| Resource | Retained identity | What changes it |
| --- | --- | --- |
| Prepared classic artwork/font | Face's worker path/program plus asset index, exact path/kind, requested frame, target size, crop and wallpaper/control role | A changed request requires its own prepared key. A different path/program creates another worker. |
| Source sprite strip | Source-local path, image metadata and target size | Changed descriptor/size or bounded strip eviction. A newly requested frame of a resident strip does not reread/redecode its PNG. |
| NativeUI package | The current NativeUI State's Package, font identities and normalized-name image cache | Reconstructing State starts package/resource preparation again. This test slice does not change that lifecycle. |
| Sample PCM/stream pages | `Pcm` clones share one `AssetId`; a separately constructed revision gets another ID. Stream pages are addressed by asset identity/page within their owned stream store. | A real source revision is distinct. UI asset identity does not authorize sample reuse or suppress an authored source replacement. |
| New-generation classic art handoff | Equal complete `ResourceRevision`, exact selected resource path and program | Missing/incomplete authority, changed namespace/stat identity, source or program disables the handoff. |

`ResourceRevision` includes the sorted discovered namespace and file length,
mtime, and Unix device/inode/ctime identity where available. It is deliberately
not a content hash. Validation happens on the loader side, not during painting.

The prepared cache has a 64 MiB budget; Source's strip cache and NativeUI decoded
images each have a 48 MiB budget. "Load once" therefore means once while the
exact authority/key remains resident. Presentation changes can deliberately
release bitmap strips; evicted or newly visible/cropped/scaled keys may need new
work. A structural widget change can require a fresh interaction/native owner
without invalidating unchanged immutable classic artwork.

## Source-free regression boundary

`src/ui/part/asset_retention_tests.rs` exercises the actual private Face
publication path and the production preparation worker and CPU painter. It uses
synthetic PNGs, an independently authored valid NKI wire fixture and the
repository's bundled font, with no installed library, plugin, audio device or
vendor payload. The shared authored-fixture helper keeps temporary files on the
dev drive locally; the serialized runner also sets dev-drive scratch/cache roots.

The A → B → A case changes menu semantic selection, menu row text/visibility,
arrow policy, control value/strip frame, label text, style colour, parent/layout,
widget visibility, enabled state, opacity and z. Every state is compared with a
fresh complete Interface, resolved geometry, actual surface geometry and exact
CPU pixels in both Bitmap and Vector presentations. A must return exactly, and B
must produce different pixels. Worker, image and font identities and actual
read/decode counters must remain stable. The first B strip-frame preparation may
complete a new prepared key while reusing the already decoded strip.

Interface publication and committed control readback are separate inputs. The
test first checks that a graph-only publication preserves live control values,
then feeds the explicit A/B/A committed values through the same `values.extend`
boundary as the production part view. The fresh reference receives identical
scalar/typed inputs before preparation. Exact pixels are compared without a
tolerance. An initial unequal-input oracle failed at the knob/menu and was
corrected at this test boundary; it did not justify a painter change.

The generation case appends a newly bound control under the same validated
resource authority. It checks a fresh generation/drafts/values alongside retained
worker and decoded font/art. A replaced resource file then rejects the handoff;
an independent worker must publish the new pixels and settle without repeated
reads. The old owner's immutable image remains unchanged. Deterministic
same-key/old-epoch result rejection is separately covered by
`picture_worker::tests::stale_completion_preserves_same_key_pending_in_new_epoch`:
the old result cannot remove a current request or admit stale pixels.

The existing shots-only `Scan` provides actual resource lookups/encoded bytes,
image decode and font-parse counts, completed keys, evictions and requeues. These
tests use those counters directly; they add no production per-frame telemetry.
Resource lookups do not count namespace discovery's internal file reads.
They do not certify original Kontakt/Falcon pixels, arbitrary callback state,
NativeUI package transfer or sample-payload reuse.

`picture_worker/retention_tests.rs` also holds an unchanged wallpaper
request while its real decode completion is held in a test-instance channel.
Changing only the presentation, or appending an unused descriptor under the same
filesystem authority, originally advanced the worker epoch and reissued that
pending key: two resource reads/decodes and one accepted prepared completion.
The phase-two correction below supersedes that characterization. This non-strip
crop has no Source strip-cache shortcut. Its final production painter pixels
must equal a fresh full-canvas source.

## Settled-publication correction and validation

The cache-hit regression was run against unchanged production first. After one
real image read/decode and accepted key, four identical settled calls reported
zero unchanged calls and emitted four duplicate prepared entries. The old
`changed = !self.unchanged` / `self.unchanged = !changed` feedback kept the flag
false forever, forcing `Assets::prepare_owned` to clear/reinsert its maps even
when bytes and pixels stayed unchanged.

The correction derives dirtiness from the first call and actual metadata,
wanted-key, result and batch transitions. The same four calls now report four
unchanged calls and emit zero entries. First empty owners, unchanged in-flight
requests, later completions, metadata changes and clearing/restoring resident
keys retain their required publication behavior. Epoch/cancellation policy,
resource keys, budgets and native owner lifetime are unchanged. This measures
avoided publications, not CPU time or a native loading speedup.

Final source-free CI tests use `--profile ci --locked --offline -j2 --features
shots`, `target-cpu=x86-64-v3`, strip disabled and serial test execution. One root
test artifact (`9679830bcfd6b149d6392a75e814df4c54e453b689cc1a85ed8e4c6621ecf3a8`)
passed these disjoint filters:

| Filter | Passed | Ignored |
| --- | ---: | ---: |
| New Face retention | 2 | 0 |
| Picture worker, including three new tests and stale-result regression | 14 | 0 |
| Picture resource service | 3 | 1 |
| IR painter/control tests | 43 | 2 |
| Existing Face controls/publication | 14 | 0 |
| Snapshot admission | 6 | 0 |
| Product snapshot reuse | 8 | 0 |
| Total | 90 | 3 |

The asserted A/B/A totals remain **4 resource lookups / 3 image decodes / 1 font
parse** for Bitmap and **3 / 2 / 1** for Vector, including B's different control
frame and return to A. A changed file authority starts a separate worker and
settles at 4 / 3 / 1 without additional work on repeated preparation. Failed RED
and unequal-input-oracle receipts are retained alongside GREEN evidence.

Private logs, source/index hashes, test-artifact hashes and compact numeric
receipt: `PluginArtifacts/kontra-diagnostic-fixes/ui-asset-retention-20261015`.
The phase-one collector covered tracked workspace sources and the explicitly
owned new files. It omitted unrelated, untracked compiled snapshot-admission
modules, although their six tests executed. The phase-two collector below binds
those modules prospectively; it does not supply missing historical hashes.
All phase-one final commands observed stable hashes within that recorded scope.
No real-library, native
host, plugin build, audio playback or performance benchmark was run for this
slice.

## Pending requests and duplicate dispatch

Before the phase-two correction, a held real wallpaper completion reproduced
**2 reads / 2 decodes** for both an unrelated asset-roster append and a
presentation change. The new wanted keys are now computed before deciding
whether a metadata change needs cancellation. A nonempty pending batch retains
its epoch only when every exact pending key remains required. Identity includes
asset index, descriptor, frame, target size, crop and wallpaper/control role,
within the same worker/resource owner. Both cases now perform **1 read / 1
decode / 1 accepted completion**, then no additional work after settlement.

If any pending key becomes obsolete, the whole batch still advances its epoch.
The tests change index, path, metadata, frame, target, crop and role separately;
each rejects the old result and accepts only the new geometry. The existing
same-key stale-result test retains its assertions with a genuinely obsolete
second request: neither old completion may clear the current pending key or
publish pixels. Empty-pending invalidation and wanted-only page/resize behavior
remain unchanged. This is a whole-batch condition, not selective cancellation.

A separate authored dispatch regression used three image uses with asset
indices **0, 1, 0**, where both asset descriptors were equal. The previous sort
omitted the asset-index tie breaker, so equal exact keys could remain separated:
**3 dispatched / 2 unique / 2 pending**. The tie breaker makes exact keys
adjacent before deduplication, yielding **2 / 2 / 2** without merging distinct
indices or changing key equality.

The same root test artifact
`8d3f88231c2bef69720003e8059d423fa623ecd82110e1ffe48fdf49ff482c88`
passed the preceding seven disjoint filters with **92 passed / 0 failed / 3
ignored**; the worker filter now contains 16 passing tests. Full A/B/A models,
effective committed values, resolved geometry and exact Bitmap/Vector pixels
remain equal to fresh references. These are avoided resource operations and
duplicate dispatches, not a measured CPU or native loading-speed improvement.

Phase-two provenance uses tracked **and untracked nonignored** regular workspace
Rust/manifests plus owned paths, excluding symlinks: **1,170 files**, including
both new snapshot-admission modules. All GREEN commands had identical source,
HEAD and index maps. During the first RED command only HEAD advanced for a
separately tested commit; its recorded source-byte maps and index were equal.
Both original RED receipts are preserved. The compact private receipt is
`ui-asset-retention-20261015/phase2-evidence.json` in the artifact directory above.
