# Kontakt snapshot resource ownership

Snapshot convolution indices belong to the snapshot's other-files table. The
previous importer retained snapshot effect bytes but discarded that table, then
looked up their indices in the base instrument. Equal integers could therefore
select an instrument filename or a different response. This was a resource
ownership defect, independent of the remaining convolution DSP laws.

`SnapshotState.other_files` now retains modern and legacy tables. Translation
records each rack/bus occurrence actually replaced by the snapshot. Convolution
requests carry their physical group/slot/generic address as well as the native
index. Replaced effects use the snapshot table; unmatched base effects keep the
base table. Missing, empty and malformed authorities never fall back across
namespaces. Script-only snapshots preserve the base effects. The existing base
NKR container authority remains available for authored resource member paths.
Impulse cache recipes pair the physical request with its resolved source; they
are no longer joined by the integer index alone.

The synthetic production-path fixture uses the same index in snapshot insert and
base main racks, with different source PCM. It verifies each physical request,
shaped impulse and recipe separately. Other fixtures cover partial rack/bus
overlays, absent/empty/missing indices, script-only snapshots, modern/legacy table
decoding and corruption without legacy fallback.

Read-only installed checks (no sample playback or payload files):

| Snapshot | Result |
| --- | --- |
| Analog Strings, 9001 Punch | Two responses resolve through the snapshot's two-entry table. Both match independent exact-source decoding and shaping in memory. 641,330 frames in total; shaped PCM FNV64 `56dbcb57118c407d`; no missing response. |
| Morphology Evolved, First Steps | Snapshot index 1 identifies an unavailable external response. Zero decoded responses and an explicit missing dependency remain; no base-table substitution. |

The normal Kontakt library suite passes 231 tests with 9 opt-in tests ignored.
Installed checks use the ignored
`owned_snapshot_impulses_resolve_their_own_file_indices` fixture and retain numeric
counts/hashes only. These checks establish resource identity and the existing
shaping path; they do not establish complete native convolution equivalence.

Snapshot lifecycle is a separate boundary. The official
[snapshot command contract](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/general-commands)
distinguishes modes 0/2, which rerun initialization, from modes 1/3, which retain
live setup and run the persistence callback. The importer models the policy but
the full-read product path still reconstructs scripts. Applying snapshots to a
live same-base instrument requires preserving the appropriate script/UI state;
skipping initialization for every snapshot would violate the other modes.
The same native contract states that snapshot loading resets the audio engine
and active events, which is distinct from avoidable load-time UI rebuilding or
long audio gaps.
