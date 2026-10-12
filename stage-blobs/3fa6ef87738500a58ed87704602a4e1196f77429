# Editor close/reopen ownership

READY product source: `3197d008a945407a404a5ec77f60bb2a6618b85b`.
Branch: `v2/fix-editor-cycles-ready-344`; parent batch: `845d82ae`.

Closing a window now releases its memoized render closures, retained scene,
animation ghosts and resolver on the GUI thread. The model and audio engine
can remain alive. Reopening builds a fresh canvas. The native window driver
uses the existing `Ui::close` boundary, already used by the plugin editor.

The four-cycle regression first retained 8, 16, 24 and 32 MiB of canvas data
after close and model destruction on a different thread. It now retains zero
at every checkpoint. The enhanced check also verifies release before model
destruction and canvas reconstruction when the same model reopens.

Validation receipts: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w3-editor-cycles-344`.
`ACTUAL-RED.log` records the executed failing test; `GREEN.log` and
`RELEASE-MEMO-GREEN.log` record passing original and enhanced checks.
`ACTUAL-NEIGHBOR-CLOSE.log` checks both close callbacks, `KEYBOARD-CLOSE.log`
checks keyboard release, and `AREA-NO-RUN.log` records the root shots compile
gate. Earlier zero-match filters are excluded from validation.

The shared-code fix touches `vendor/mui/src/host.rs` and
`vendor/mui/src/ui/mod.rs`; its regression is in
`vendor/mui-baseview/src/tests.rs`. It changes neither resource resolution
nor authored pixels. The parent memory batch's Conflux scanner reported
Original OK before and after with all three pixel hashes identical to
`58d035b6`; `w3-editor-memory-344/PIXEL-CHECK.json` records that verdict.

## Separate RSS investigation

The toolkit regression does not explain Conflux's large RSS growth: the
current product does not use `Ui::memo`. An instrumented frozen 381 host
returned to 29 threads after each of four destroys; live allocator bytes
grew only 0.396 MiB while free arena capacity grew 164.955 MiB.

The matched own 344 host pair retained the same selection and agreed on the
viewport at every reopen, but RSS still grew with this fix (276 to 443 MiB
closed). `MATCHED-PAIR.json` records exact sources and artifact hashes. Load
status remained partial; authored-package readiness was not exported and
settling was fixed at four seconds. Quiet admission and performance are
UNKNOWN. No RSS reduction is claimed.

The optional GNU page-release experiment `bd0c5bf1` is parked separately on
`v2/w3-editor-rss-page-release-wip` and excluded from this READY branch. It
reduced observed closed RSS, but failed the unchanged 5 MiB spread check
(13.84 MiB). Held-note diagnostics remained finite and audible; timing
admission is UNKNOWN. No allocator tuning is introduced by the READY fix.

Parked symptom: closed RSS remains non-flat despite near-flat live bytes.
Best hypothesis: repeated graph/render allocation leaves free arena pages resident.
Next step: distinguish post-warmup arena growth from retained owners with readiness-aware host checkpoints.

No full suite, corpus gate, Windows validation or quiet performance acceptance
was run for this batch. No install or release was performed.
