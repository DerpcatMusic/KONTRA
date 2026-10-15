# KSP snapshot setup identity and capture boundary

Una Corda Felt explicitly sets snapshot mode 1 in physical script slot 0, line
13; Analog Strings does so in physical slot 2, line 42. Metadata-only reads used
the production container/resource authority without running initialization. The
source digests are respectively
`b7bbcb44b60baa3bfd0c01e23d6e9961a4b81e176c1d831872b62cfc37d69a7e` and
`5df3f62429a96f366f9250b29e7b370712fbfe00ae6448d8adf4453bafdbcee1`.
Una's inline text uses the production Windows-1252 fallback; Analog's is valid
UTF-8. No other physical slot contains a snapshot policy setter.
The sanitized private receipt is `ksp-snapshot-lifecycle-raw-metadata.json`.

The official [snapshot policy](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/general-commands)
acts globally across five slots. Modes 0/2 rerun initialization; modes 1/3 retain
setup and run persistence handling. Modes 2/3 save script state only. This differs
from choosing a fresh default independently for every script slot. The
[instrument-persistence contract](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/variables)
also distinguishes values saved with the instrument from snapshot variables.

The compiler now exports:

- `Script::setup()` / `ScriptView::setup()`: SHA-256 source, layout and context
  identities, physical slot and sample rate. Layout covers actual bindings,
  domains and authored global storage; context covers preprocessing, physical
  groups/zones, engine lookups and the performance view. Native engine values
  remain independent. These identities are in-process gates; they do not replace
  base-file, generation or resource-authority checks.
- `Model.snapshot_mode_write`: the last executed initialization setter, including
  source offset. An absent write is distinct from an explicit zero. The
  five-slot `snapshot_policy` selector orders physical slots, ignores absent
  writes and rejects duplicate/out-of-domain slots.
- `capture_roster`: caller-budgeted numeric/text global and widget addresses,
  private numeric/text-property store capacities, shared PGS numeric capacity,
  and persistence callback program identities. It excludes polyphonic/note
  storage, callback PCs/registers, lowering scratch and drag/drop payloads.

Initialized/cached state cannot be compiled against a different source. Restoring
cached state still performs positioned source admission before rejecting source
identity mismatch; no cached values are applied first. Missing source identity or
inconsistent policy metadata is rejected.

The synthetic live-state oracle establishes both the useful existing capability
and its limit. Persistent-only capture loses an edited private global (333 becomes
its initial 33) and private text. The source-bound full **value** roster restores
these, and the widget value, without audio allocation or destruction. However,
the target's private UI property store and model still contain their initial hide
and picture state. Listing those stores is not capturing them. The fixture
requires that difference to remain visible; no partial restore is presented as
complete setup preservation.

A future live snapshot transaction must capture those stores and the current UI
model at a coherent plan/revision boundary, apply only snapshot-persistent saved
values with the correct menu-index law, retain instrument/nonpersistent setup,
and run persistence once after commit. It must validate exact base/source/layout
and cancel stale work. Native DSP controls must follow the mode's native-state
policy rather than being overwritten by a generic old host-state recall.
Active events may stop at native snapshot commit; preparation must keep the
current part active. This checkpoint adds metadata and a bounded roster, not that
production transaction.
