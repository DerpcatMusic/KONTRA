# UI, resource and native-host consolidation review

Baseline `972b4410571bb9c3144e3af5d1800b69258f0570`, 2026-10-12.
The [scoped ledger](ui-consolidation-review.json) records **116 local refs and
18 dirty donors**, with immutable tips, scoped comparisons, exact-equality
counts and disposition reasons. It uses the fresh consolidation inventory,
not graph-unmerged counts as a missing-feature oracle. Limited-depth older
snapshot rows explicitly remain unverified; their source and dirty bytes are
preserved by the coordinator before checkout deletion.

No build, Cargo, compiler, protected instrument, sample library or native plugin
was invoked by this recovery agent. Rustfmt parsed the changed Rust sources;
`git diff --check` passed. These are source checks, not execution or native
acceptance. All code changes were made in the assigned isolated worktree.

## Recovered substantive scopes

| Recovery | Historical scope and resulting behavior |
| --- | --- |
| `a1a6f011` | `v2/fix-viewmodel`: a worker-owned, immutable-source-owned MappingIndex retains group counts, links, source-zone identities, geometry, fitted bounds, first selections, names/search and interval-key membership. The editor shares it across reopen, virtualizes group/articulation rows, and caches map/waveform canvas paints. Current keyboard paging, group search, condition captions, audition release and zone identities remain. |
| `ee906d63` | Bounded corrections from pending MUI `65ab5`: retained memo captures are removed from the thread-local table before destruction; release, close, drop and headless parking use that boundary. Invalid headless times cannot poison Duration construction. The current renderer and native ownership fork remain. |
| `c52d2cb5` | `aceb2f9e`: UVI authored values preserve float32 defaults/writes, display-range independence and integer truncation. Numeric overflow leaves state unchanged; momentary buttons expose no scalar value setter, toggles require booleans, renderer edits convert toggle payloads, and parameter widgets use the catalog default. |
| `2063f8f2` | `67fed2df` through `99dfa1e1`, with `f5307c87`/`9d864f11` guards: shared Rect/Size carry fractional geometry through parent accumulation, renderer, headers, generated views and sparse publication. The integer Kontakt constructor remains exact, PNG strip frames retain integer division, and allocation boundaries round up within current host limits. UVI raw object identities/current parents and authored child ordering drive paint order without renumbering declaration/control identities. Malformed trees, nonfinite/negative dimensions and accumulated overflow produce explicit snapshot diagnostics. |
| `96ef7f86` | Corrects two central regression seams: the worker/audio fixture loads an authored UVI instrument that references its generated small WAV, and the large mapping fixture switches actual map content to Sound Controls before testing reopen. Plain WAV's absent Instrument metadata is no longer mistaken for failed index publication. |
| `ed1c1bd1` | `8e767b52`/`a48fa1df`/`226b7ac7`: authored Falcon controls use Ctrl/Cmd fine and Alt reset; Kontakt preserves Shift fine and Ctrl/Cmd reset. Knob, slider, ValueEdit, table-cell and XY-fader numeric entry shares finite/ratio/step/clamp/commit/cancel rules. Mapped accessibility edits rebase their current pointer grab. Current stock captions, authored menu backgrounds, slider/knob orientation and retained rendering are preserved; numeric editor revisions invalidate IR/generated retention. |

Geometry validation allows finite negative x/y, including nested offscreen
placements. Nonnegative constraints apply to width and height. An explicit
negative nested-position regression documents that boundary; page and viewport
clipping remain renderer operations.

## Already current or superseded

`perf/ui-gpu-limits` already supplies component read tracking, Lua graph/lowered
node reuse, event indexing, transparent-layer skip, one-shot GPU retry and
adapter-supported large buffers. `fix/knob-drag-vector-kind` already supplies
retained KSP widget runs, interned asset identities, unchanged picture
preparation and nearest decoded strip frames. Conflux retained/glyph branches
have current bounded retention, eager size reads, font/grid invalidation and
replaced-session successors. These commits must not replace newer renderer
code or be used to claim the installed Conflux blank result is resolved.

`wp7-wp8-ui-diag` and `diag-copy-specific` have current warning-class grouping,
positioned safe fault detail, UVI module paths and setter-type explanations.
The old `gpt/script-view` tip would remove actual report producers; current
source is retained. Browser search, title/status separation, banners, cached
category order, protected-reader artwork invalidation and work counters have
current source/tests. Dirty browser redesign assertions that assume one
combined library/preset query are superseded by separate query ownership.

The dirty NICNT `read_index/read_member` prototype is superseded by bounded
`read_table/read_item` and retained physical IDs. Dirty widget recall attribution
is superseded by submitted-owner and individual array-cell accounting. W13's
primary-envelope and watched-part/group source bars were already recovered in
`533f6785`. The coordinator's backend agent owns the genuinely missing dirty
`fix-load` bounded capture/readback seam. Historical ignored UI/resource/timing
probes remain preserved, unrun, and distinct from current acceptance.

## Material limits and preserved candidates

The pending MUI sibling pin switch to `65ab5` and Windows DXC dependency addition
were **not** replayed as a package-only change. That upstream revision changes
95 files, including GPU initialization/pipeline cache, truce and native OS
callback APIs; current application host and renderer retain custom ownership,
CPU fallback, accessibility and retry hooks against `dcf079`. Two bounded
stability contracts are recovered, but the full framework migration still needs
a coordinated native-adapter review and platform execution. The exact pending
ref is preserved, not labeled completely integrated.

The historical Dynamics-in-Settings placement proposal conflicts with the
current performance-strip design. Its next-load selection/recall/reset behavior
is current; the placement candidate is preserved without changing Graphite Rack.
The native-widget adapter was explicitly parked pending product policy and was
not activated. Limited-depth older pre-v2/platform rows retain their source
refs; this review does not establish every historical behavior integrated.

Conflux's installed blank paint and Pacific/Vista's absent authored pictures
remain separate unresolved evidence. No replacement artwork, fake native
pixels, relabeled omissions or native-equivalence claim was introduced.

## Regression targets and execution status

The coordinator reported **630 passed, 2 failed, 29 ignored** for the earlier
Mapping/MUI/capture host cohort. The two mapping fixture failures motivated
`96ef7f86`; its rerun was pending when this report was written. The newer value,
fractional geometry and gesture cohorts also await coordinator execution.

Focused targets are:

- `sampler-ui-ir`: geometry/order validation and `publication::tests`, including
  fractional reparenting, explicit empty/source order and identical publication.
- `sampler-uvi`: `values`, `projection`, `image_geometry`, `ui`, `ui_assets` and
  `enums`; projection includes 6,481 widgets, malformed lists/cycles, bad
  dimensions/overflow, negative nested positions, edit/readback/restore and
  stable source/control identities.
- Root UI: `mapping_worker_index_54k_keeps_identity_and_bounded_key_membership`,
  `mapping_workbench_54k_caches_idle_paint_and_reopens_without_reindexing`, the
  worker loader/audio/noop/clear fixture, existing mapping navigation/search,
  `w10_fractional_uvi_child_order_paints_callback_result_at_device_scale`,
  `gesture_policy_tests`, generated descriptor and retained-widget tests.
- Vendored MUI: memo lifetime release/close/drop and next-frame rebuilding,
  invalid headless clock, plus existing native-host close/reopen tests.

The geometry pixel witness uses authored temporary textures and the current
KONTRA renderer. It establishes local producer/painter behavior only. None of
these targets is a native Kontakt/Falcon comparison.
