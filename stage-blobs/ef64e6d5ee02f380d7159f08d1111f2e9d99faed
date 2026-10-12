# Harp load panic: fixture-only

The untouched installed Vista Harp loads on exact shipping source
`f199980d68ddef02634f07109a33de5bbe3470cd`, both with scripts disabled and
enabled, after key-60 compaction. Both checks pass from the original 2,000
zones. The owned `w15-harp-shipping` tree has no production edits.

Receipt: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w15-harp-mode3-20261010/shipping-load.log`
and `shipping-load.exit` (0), corrected per-worktree wrapper.
The ignored regression is `shipping_harp_compacts_without_fixture_mutation`.

The failing mode-3 null witness directly removed all but group 0's 100 zones
from `instrument.zones`. Its source-zone mapping still described the original
2,000 zones. The subsequent `load_read` called `retain_zones`, whose remapping
at sampler-ir/src/lib.rs:692 indexed that stale zone reference. This is a zone
mapping error in the fixture, not a processor-compaction crash in shipping.

Fixture repair `99c04de3fac015a3357183d817946e0b3762f838` uses `retain_zones`
and remaps the parallel `Kontakt.locations` asset list using its returned
indices. The existing production compactor is unchanged; no bounds guard is
introduced. The fixture also accepts the frozen CLI's IEEE float extensible
WAV header, after checking its subtype GUID, channels, rate and bit depth.

This establishes shipping load safety for this instrument and these two
script settings. It does not establish mode-3 audio parity; the repaired null
is a separate pending measurement. Its frozen v1 WAV remains only in RAM.

The repaired null next exposed another fixture-only validation error: clearing
all buses left unused chains scoped to bus 0. The dry fixture now preserves bus
owners, removes their chains/sends, routes groups to master and clears voice
send taps. A bounded signal trace is enabled for the pending null; a failure
prints stage metrics only. The latest fixture edits await their own area
no-run after W0's quiet window. The shipping load verdict above is unchanged.
