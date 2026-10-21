# KSP UI paint order and bounded activation evidence

Current Morphology Evolved / Dual Splangs resolves its saved snapshot to the
original NKI and renders its authored 960 × 630 interface. The older installed
binary's reported blank editor has not been reproduced in that binary. This
witness exercises the current checkout's production loader and renderer, without
a plugin, DAW or audio device; it is not a native Kontakt parity capture.

## Two independent boundaries

Classic KSP painting previously sorted only by global z layer and declaration.
It now also applies the documented widget-type priority within each layer.
Explicit native paint order, inherited hiding and relative parent coordinates
retain their own rules. The [NI control-parameter reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/control-parameters#size--position-and-look)
defines that ordering. An authored overlapping label/button fixture verifies
both resulting CPU pixels and pointer ownership; an explicit higher label z
still wins. IR fixtures also retain native explicit ordering and parent hiding.

Separately, a wait-capable `persistence_changed` callback is scheduled for plan
activation rather than executed fully by the initializer. An initial-only
preview therefore cannot establish the later interface state. `--settle-ui`
loads once through V2Loader, retains its initial interface, installs the real
Part/V2Core and services the same read-only KSP effect outbox while advancing
bounded 128-frame blocks. It then paints the observed state with the same
production painter. No synthetic note/controller/transport/gesture input or
replacement engine metadata is supplied. Authored startup/listener work can run;
output is measured and discarded.

The bounds are 96,000 audio frames at 48 kHz, a five-second stepping threshold
and 65,536 applied effects. Wall/effect checks occur between blocks/effect
drains; the read-only launcher's process timeout is the outer bound. Faults
already removed from the active scheduler remain observable through nonconsuming
FaultInbox counts. Unsupported services, unresolved callbacks and conversion
gaps keep the evidence partial. The default `--ui-only --render` path still
constructs no Runtime and explicitly records deferred persistence work.

## Real snapshot receipt

The source-free 2026-10-14 raw lifecycle receipt used inspector SHA-256
`f699a806bf52f67ae0131f2f66142a6e99294b51bfc5a79eb224395f2c8d58c9`.
Other team builds/probes were held for this inspection; OS caches were not
evicted. Original source, artwork and PNGs stayed in the private RAM viewer.
Numeric receipts live on the dev drive under `PluginArtifacts/kontra-diagnostic-fixes`.

| Observation | Compiled initial state | After bounded activation |
| --- | ---: | ---: |
| Widgets | 671 | 671 |
| Inherited-visible widgets | 140 | 113 |
| Visible positive-area widgets intersecting page | 94 | 70 |
| Visible widgets extending outside page | 35 | 35 |
| UI properties still unmodeled | 2 | 2 |
| Actual asset lookups/decodes | 50 | 46 |
| Missing, failed or pending assets | 0 | 0 |
| Bitmap/vector CPU witnesses | Both OK, 960 × 630 | Both OK, 960 × 630 |

The production loader took 431.4 ms; 750 blocks advanced two seconds of runtime
in 15.9 ms of host wall time and applied 4,151 UI effects. Seven raw widget
records changed. In particular, three drawer child panels became hidden and
their 27 descendants inherited that hiding. This is a lifecycle correction,
independent of the type-order correction. Layout is not automatically moved or
repaired. Core faults, underruns, nonfinite output, capacity drops and active
voices were zero; discarded output peak was zero.

One plan-owned callback still waited at the frame bound. The raw receipt keeps
its status partial and does not infer its kind from program number or PC.
Retained compiler metadata now distinguishes authored callbacks from generated
periodic listener drivers, with explicit unclassified/omitted rows. A recurring
driver can legitimately remain pending after persistence work has ended.

A classified finite-only control run (`f59319d1…f2373d`) proved program 0 was
`on persistence_changed`, already finished, while program 2 was the generated
listener driver. Stopping immediately after finite work therefore advanced zero
frames, applied 154 effects and changed only three widgets. It did not reproduce
the later drawer hiding. This rejects finite-only readiness as sufficient UI
observation for this instrument: the periodic callback performs necessary later
UI writes. That receipt is preserved separately, not silently replaced.

The runner now retains the exact driver-to-callback mapping at compilation.
Before end_block drains terminal outcomes, it observes source-free completed
listener program IDs. Early stop requires each active driver's callback to have
completed at least once, its UI effects drained and no pending finite or
unclassified work or retained fault. A timed authored fixture requires a later
listener hide write; a faulted listener cannot count as a completed cycle. All
original bounds remain. One observed cycle is not proof of future visual
stability.

The final first-cycle receipt (`363daf0e…df7e92`) observed exact driver program
2 dispatching authored listener program 1. One 128-frame block (2.667 ms of
runtime time) and 161 effects reproduced all seven widget changes and inherited
visibility 140 → 113. There were no pending non-background callbacks, unknown
rows, faults, cold starts, underruns, nonfinite output or active voices. The
periodic driver remains explicitly active. The 671 widget records, pages,
assets, styles and unsupported records exactly match the earlier 750-block
observation; **both Bitmap and Vector PNG bytes also match exactly**. This
validates the bounded first-cycle policy for this witness without a blind
two-second advance. It does not prove later visual stability or native parity.

The final loader took 613.3 ms and stepping 0.248 ms; total diagnostic inspection
including four PNGs took 2.829 seconds. A shared core test compilation was active,
so these wall clocks are not an isolated before/after performance comparison.
The numeric receipt is
`morphology-dual-splangs-first-listener-cycle-evidence.json`; the raw two-second
and finite-only receipts remain separate for review.

The two remaining UI properties are indexed automation IDs and cursor pictures.
All requested assets decoding is not proof that these properties, later
gestures, meters or the complete audio model are faithful. The raw loader also
retains 1,077 source-conversion findings; these are repeated occurrences, not
1,077 distinct defects. The classified collector reports their feature-family
and reason counts without authored values or diagnostic text.

| Conversion feature family | Occurrences | Disposition |
| --- | ---: | --- |
| Filter | 904 | UnknownLaw |
| Keys beyond runtime pitch range, narrowed | 127 | InvalidValue |
| Wavetable source | 30 | NotModeled |
| Saved release/MIDI/voice state and v4 selection words | 7 | NotModeled |
| Script Warning | 5 | NotModeled |
| Script interface | 2 | NotModeled |
| Tape Saturator | 2 | NotModeled |

The colored lower LEVEL strips are volume sliders bound to controls, not live
meter observations. For example, a 15 × 104 slider at (282, 470) is distinct
from the two 4 × 98 meter columns at (299, 473) and (307, 473). The bitmap
presentation uses the authored slider strip; vector presentation draws a fader.
The offline painter has no live meter provider, so empty meter columns do not
establish native meter transfer law or live ownership.

Total inspection was 2.652 seconds, including two interfaces and four encoded
PNG witnesses. PNG encoding alone took approximately 134 ms per witness. These
are diagnostic-process costs, not plugin startup or native-host timings. The
[Zero-G product page](https://zero-g.co.uk/products/morphology-evolved) supplies
baseline artwork/layout evidence with another preset/drawer state; it is not a
same-preset native-host comparison.

## Regression scope

Six authored lifecycle fixtures cover completed persistence writes, pending work
at the frame bound, an actual negative-wait fault retained after end_block,
exact multi-script program-offset/driver mapping with a necessary timer-delayed
UI write, a finite fault alongside an ongoing driver and a faulted first
listener. Division by zero
and out-of-range array access are nonfaulting KSP operations and are not used
as false fault fixtures. Python checks retain the read-only RAM sandbox,
selected program/base and private pixel routes. Original vendor scripts/images
are not committed as fixtures or reports.
