# UVI/Falcon UI health and fixes

Scope: the local 660 `uvi-program` corpus rows, UVI frontends and their shared
presentation/control services. This report distinguishes loading a Lua host,
constructing a complete UI, resolving assets, painting a native scene, and
verified parameter behavior. None alone establishes Falcon compatibility.

## Baseline recovered after the server restart

The interrupted baseline completed **239/660 programs (36.21%)**. All 239 hosts
loaded and exported UI IR. Retrospective findings show 12 initialization-error
events in four programs: 235/239 (98.33%) initialized without a recorded error.
The baseline collector did not paint scenes. Do not extrapolate these results to the remaining
421 programs.

| Failure, ranked by occurrences | Programs affected | Occurrences |
| --- | ---: | ---: |
| Interactive widgets have no control-service binding | 239/239 | 392,199 |
| UI image references fail the old audio-resource lookup | 239/239 | 43,207 |

Baseline binding coverage was **0% (0/392,199)**, and resource-resolution coverage
was **0% (0/43,207)**. Unsupported widget/property counts were not reliable:
the old widget proxies absorbed unsupported operations and ignored assigned
`bounds`. No CPU render or font-parse coverage was measured.

Private metadata-only records and the frozen baseline executable are in
`~/.cache/kontakto-gpt-uvi-ui/`. Scripts, XML, image/font bytes, sample bytes and
keys are neither cached nor committed.

## Implementation

- Stateful Lua widgets retain coherent bounds/position/size, scalar booleans and
  floats, menus and multi-state cycling, fractional Table cells and original XY
  parameter identities. Edits invoke the originating script's real callback on
  its worker; yielding in a widget callback reports an error.
- The bounded edit handoff and immutable presentation snapshots connect UI and
  v2 core control values. Momentary buttons emit one activation per click or
  keyboard activation. Parents propagate visibility, disabled state and opacity;
  viewport origins and clipping preserve nested layout.
- Bank-local images and TrueType fonts resolve through a bounded UVI resource
  service; ambiguous suffixes and other-bank references are rejected. Resource
  bytes remain in memory. The native renderer consumes the authored artwork,
  button states, Tables, XY axes, menus, fonts and program meters.
- Automatic widget state is captured separately from custom `onSave` Lua data,
  including binary strings. Restore applies widget callbacks before `onInit`,
  then custom `onLoad`; cyclic/userdata state is rejected. Plugin state carries
  the source identity and refuses malformed or oversized snapshots rather than
  silently discarding them. A changed preset cannot inherit another preset's
  saved widget values.
- The resumable collector flushes one metadata record per UVI program, defaults
  to 40 items and 240 seconds per invocation. Its root adapter additionally
  exercises the production asset loader, layout and CPU painter without saving
  proprietary images or decrypted sources.

## Validation and census continuation

UVI and UI-IR `cargo test --no-run` passed before formatting cleanup. The first
six authored UI tests found a shared Lua multi-return bug in numeric restoration;
`tonumber((string.gsub(...)))` fixes it. The final root `cargo test --no-run` passed after fixing the meter adapter type.
All six authored UVI UI tests pass. The native renderer test also passes for
mouse hold/release and keyboard activation, with one momentary callback per
activation. Native mapper round trips and strip selection pass for all ten
documented mapper types; normalized Lua edits include QuinticRoot. Constructor
defaults remain separate from current values for reset. The optimized census, matched before/after measurements and ranked
residual failures are pending. This is a progress report, not a completion claim.

Run builds and every census shard through `~/.cache/kontakto-heavy` from this
worktree. Use the wrapper's per-worktree target, without setting target variables
or slot counts. At this report's revision, the collector selected the cached verified reader with
`KONTRA_UVI_READER=~/.codex/cache/kontakto-uvi-official-reader/app/UVIWorkstationx64.exe`.
Its SHA-256 is the loader-verified official Workstation 4.0.9 hash. An initial
40-item after run without this selection had only reader-access failures and is
excluded from UI coverage. The current runtime uses bundled native namespaces
and no longer reads this executable. Compile the native render collector with:

```sh
~/.cache/kontakto-heavy cargo build --profile ci --example uvi_ui_health --features shots
```

Run the resulting `ci/examples/uvi_ui_health` with the UVI-only `uvi-items.tsv`, a
fresh output JSONL for that binary revision, and a maximum of 40 programs, wrapped
in `timeout 285s` inside one heavy call. Each invocation resumes from completed
IDs in that output. It distinguishes `loaded` (host), `ui_loaded` (no initialization
error), missing resources/fonts, unbound widgets, unsupported documented meanings,
validation failures and image/font/layout/CPU-render errors. Record percentages
on the 239 matched baseline IDs separately from the full 660-program results.

## Boundaries still requiring evidence or another owner

- Engine parameter definitions currently manufacture 0..1 ranges, omit types,
  enums and mappings, and do not establish bool/string parameter setters or
  automation/modulation readback. Parameter widgets need the runtime agent's
  real definition and parameter-change services. Connecting a scalar UI binding
  does not prove the intended engine parameter or DSP module works.
- `getParameterConnections`, `uvi.ChordRec`, sample/impulse/data loading and other
  script runtime findings remain the UVI runtime owner's scope. Initialization
  errors depending on those APIs must be reported, not hidden as UI success.
- WaveView/sample waveform binding, file selection/drop services, SVG parity,
  non-program bus meters, full keyboard color translation, exported host
  automation and reference-host traces remain open. Their affected program
  counts await the full optimized census.
- The corpus must be checked for independent XML/skin views and font declarations;
  absence of matching XML nodes in the first 239 records does not prove absence
  across the corpus or in other installed UVI products.
