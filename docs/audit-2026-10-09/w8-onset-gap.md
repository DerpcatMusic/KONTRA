# W8 installed-to-first-sound attribution

Receipts: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w8-onset-gap-20261009`.

Frozen Kontakt v1 is 0cb7a8a0. No UVI reader, Wine, or third-party opener was used.
The v2 probe uses CI optimization without ThinLTO; frozen v1 uses release. Product
cache state and OS cache remain separate, and onset is measured after installation.

| Same cell | Frozen v1 onset after install | Original v2 32179b75 | v2 after existing W9 dirty-snapshot fix |
|---|---:|---:|---:|
| Areia Full Ensemble | 1.739 ms | 517.556 ms | 466.998 ms |
| Dolce Violin 1 | 1.721 ms | 357.407 ms | 222.929 ms |

Both new after-reuse rows are observer QUIET. Note admission takes under 0.1 ms.
Areia captures 1,283,817 persistent/widget values; Dolce captures 1,281,786.
Changed blocks spend 60–75 ms in capture/publication, whereas unchanged blocks
skip it and render in approximately 0.12–0.18 ms. Runtime voice rendering and
streaming service do not account for that copying cost. The existing W9 fix
(034a4e4f/c6db4941, already integrated in 381) is retained, not reimplemented.

The global script revision also counts temporary cell writes. The new regression
uses authored KSP with a saved scalar and a temporary note variable; the latter
must not recapture the producer buffer or rotate the coherent snapshot. Captured
writes must still publish, as must restore, widget and DSP edits. A registered
capture domain should include the production persistent/widget schema. Unregistered
core runtimes keep conservative tracking. Text/store mutations remain conservative.

Separately, the readiness regression creates a streamed voice from a yielded
callback. Before the fix, the first stream job is absent after the same block:
streaming was serviced before Runtime resumed that callback. v1's engine processes
scripts before voice commands. Settling callbacks with an empty render before
service_streaming preserves the sample clock and remaining fuel, and allows the
first page to be requested in that block. Existing offline delayed-storage PCM
verification remains required.

`PROBE_ONSET=1` enables test-only diagnostics in plugin::tests::probe_load.
`V2Core::onset_audit` is opt-in and absent from production builds. Counter order:
native ingress, Lua wake, streaming service, voice render, not-ready horizons
(count), coherent capture/publication, then native callback settlement. Numeric
metadata only is persisted by tools/audit-load.py; library data stays read-only.

Validation: both root regressions failed at their intended assertions before
fixing. The targeted core revision/array tests, five host persistence tests,
same-block readiness, delayed-storage offline PCM, widget callback and root
no-run checks all pass. Callback writes and publication remain allocation-free.
The frozen `6885c290` pair is QUIET: Areia's onset is 199.520 ms and Dolce's
152.331 ms after installation, down from 517.556/357.407 ms. Capture still
accounts for 190.132/147.140 ms before first sound. Both callbacks retain a
771,751,936-byte producer allocation, dominated by inline text capacity even for
numeric cells.

The follow-up keeps only compact addresses, atom metadata and the existing three
atomic snapshot slots after preparation. It captures live values straight into an
unpinned slot and publishes only on complete success; host serialization and the
snapshot schema are unchanged. It removes the wide intermediate copy and its
retained allocation. The authored 32,768-cell memory regression fails before the
change (12,058,624-byte producer allocation) and passes afterward (at most 96 bytes
per value including all three slots). Seven persistence tests, core revision/array
checks, readiness, offline exact PCM, widget and root no-run checks pass. Rejection
after a partial capture leaves the old slot published. Numeric writes/publication
remain allocation-free. The compact candidate timing is pending.

Text/store invalidation and actual mutations of very large saved arrays still
require a complete coherent capture. This document
makes no full v1 parity, all-14 acceptance, or release claim.
