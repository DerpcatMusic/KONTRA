# W1 view model and publication implementation

Baseline: `7e82b152` (core-v2). Retained witnesses originate from UI_LOOP `46bf47ac`, with Original-default gates from `732037d5`.

A generation retains its immutable Interface Arc. Runtime publications carry cumulative sparse overrides in the existing UI IR; only changed scripts regenerate. Applying them keeps Face, script/page, values, presentation and picture source/cache. Repeated property/key writes do not publish. Widget identities include part, source epoch and script.

One resolver chooses the serialized rack override, saved instrument preference, or global preference. Factory global preference is Original regardless of unsupported metadata. Both body and header picture menu offer Original, Vector and generated KONTRA. Generated sections and strips consume existing IR widget references and bindings. Fit uses both axes; explicit 1x, 1.5x and 2x scales are exact. Requested unsupported native views produce a load diagnostic and visible warning.

The scalar bridge now uses the native ControlClient queue and admission replies, rather than publishing unadmitted atomic values. Pending presentation is separate from authoritative readback. Source epochs guard submission, readback and effect replay. Changed native scalar revisions refresh after rendering; the 100 ms idle Watch includes mirror revisions. The typed WidgetEdit extension from W5 follows independently.

Runtime UI aliases, including set_knob_label/set_text, movement, hide/unit/default, menu mutation, skin offset and UI colour, reach the existing KSP model and IR. Alias first arguments lower to UI identity rather than the current variable value. Equal effects are no-ops.

## Measurements

Local Conflux, sample key subset 0, optimized CI profile; baseline CPU timings are not GPU or input-to-photon measurements. Full-editor harness uses 1180x900, 8 warmup + 24 measured frames and retained painter resources. Direct UI-loop harness uses 1000x600 and creates fresh painter resources. Compare each only with the same harness.

| Baseline full editor | Build median | Total median |
|---|---:|---:|
| Original | 0.305 ms | 7.202 ms |
| Vector | 0.295 ms | 6.694 ms |

Baseline full publication median: approximately 0.376 ms (8 samples). Load: 4830.653 ms, 390 controls, three interfaces. Callback: three effects emitted, two applied, set_knob_label discarded. Main-page input: 0/1 sampled drags in both modes; secondary page: 1/1. Widget gesture fixes belong to W2.

Before logs/JSON: `~/.cache/kontakto-fix-viewmodel/{before.log,loop-before.log,loop-before/conflux.json,failing.log}`. No decrypted source or assets are stored. After measurements and typed transport validation will be appended after the follow-up gate.

## Retained checks

Four synthetic loop witnesses were flipped from assertions of defective behavior to expected behavior. Before production edits, those plus the Original-default and label-alias checks failed (six failures). Checks now also cover sparse/no-op publication, restored mode precedence, header three-mode selection/editor reopen, script/page/value continuity, stale effect rejection and both-axis zoom.
