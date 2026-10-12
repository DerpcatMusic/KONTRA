# W10 UVI authored widget construction — READY

Product: `9e75ea02c87e43161d3c5ff6367d9d22795d7827`. Reference: `4bffbb18:src/uvi/host.rs:2625-2744`. The shared constructor applies scalar fields first, then `size`, `position`, `pos`, and `bounds`, matching v1 precedence. Named parents publish into the shared hierarchy. Container constructors preserve an explicitly supplied parent and expose their children; non-container constructors fail.

Two behavioral REDs (0 passed / 2 failed) became GREEN. The geometry contract checks 64 authored knobs with competing declarations. The parent contract checks nested Panel/Viewport relationships, named-parent precedence over the calling container, child membership, page coordinates and invalid container calls. Targeted class/construction/module/UI/assets checks passed **27/27**, followed by `sampler-uvi --no-run`.

The real shared-renderer test now uses global knob/slider constructors with named parents and competing scalar/array geometry. Its 320×160 page retains the declared rectangles `(30,20,32,32)` and `(90,20,32,32)`. Strip centers at `(46,36)` and `(106,36)` are red before and yellow after the authored Button callback; the label changes Ready→M through ChordRec. The pixel test passed, both PNGs were visually inspected, and their hashes equal the previous authored fixture receipts. Root `--lib --features shots --no-run` and `git diff --check` passed. The unit drained inactive/MainPID 0.

## Receipt

`/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w10-uvi-scan-344/ui-construction-READY.json`: exact product, source/log/PNG hashes, corrected target and wrapper digest. Raw logs are `ui-construction-RED.log`, `ui-construction-GREEN.log`, `ui-construction-area-no-run.log`, `ui-construction-pixels.log`, and `ui-construction-root-no-run.log`. Screenshots are under `construction-ui-shots/`.

## Open projection and bank checks

Fractional geometry still loses precision when projected into integer shared Rect fields (v1 UiBounds uses f64). V1 also retains stable widget IDs separately from authored child paint order, including reordered children and manual reparenting (`host.rs:3369`); the current shared interface has no explicit paint-order field. Those source-observed projection gaps were reported to W3/coordinator for shared API/renderer ownership. This slice does not claim them fixed.

The per-bank checklist remains 26 banks / 660 programs UNKNOWN/PARKED. Protected payloads and an official reader were not opened. Callback deadline acceptance remains open; the corrected single frozen diagnostic had zero misses and supplied no attribution event.

NEXT: numeric Unit/Mapper IDs and v1 readouts; coordinate shared authored-order/fractional-geometry support.
