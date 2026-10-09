# Reader-lane consolidation

W6 `v2/fix-tester-reader-versions` (`d372a4f9` fixtures, `ac3b55a7` source)
is superseded by W12. Do not validate or integrate that branch separately.
Validated W12 format slice remains `4fb8bd5f` + `7bc29b21` after `c1359117`.

| W6 change | W12 disposition |
| --- | --- |
| Arrays v0x11 | Covered by validated W12; retain both bounded native framing variants. Do not replace with W6's presence-only bounded reader. |
| External v0x100 / internal v0x80 nullable names | Covered by W12; add W6 internal/unassigned-source fixture cases. |
| AHDSR v0x10 | Use validated c1359117, drop W6 duplicate envelope hunk. Add opaque negative-zero/NaN tail-bit fixture; known parameter NaNs remain rejected. |
| Boolean error field context | Extra W6 fixture requires context. Owned pending patch adds context without admitting invalid booleans. |
| Compact group snapshots v1..v4 | Unique source extension, retained as one owned pending two-file patch. Its adjacent-record/source-revision matrix supplies failing-first coverage. |

`vendor/ni-file/tests/reader_versions.rs` preserves W6's four authored test
families: all array revisions/high physical slots/truncation/invalid presence;
legacy nullable internal and unassigned external assignments; opaque AHDSR tail
bits and edit preservation; adjacent compact groups for versions1..4, source
versions0x100/102/104/106, sampler/DFD modes, and malformed/truncated records.
No native bytes or names were added. These extra fixtures are **UNRUN**, outside
READY7bc29b21. They are committed locally for the next W12 turn, not pushed as a
new READY. The earlier root no-run receipt applies only to the validated SHA.

The production snapshot changes and three strict diagnostic-context additions
remain owned patches under
`/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w12-format-revisions/`:
`w6-compact-snapshot-source.patch`, `w6-modulation-context-source.patch`.
Both pass `git apply --check` against READY7bc29b21. No duplicate arrays/names or
envelope hunks are present. Shared callers were inspected: ni-file Snapshot
read/write/modulation chunks and sampler-kontakt snapshot translation. Full
source bounds are handled by the existing BParSrcMode codec; raw bytes remain
in memory and writer roundtrips retain the source/group trailer.

Validation plan: on the next direct W12 machine turn, run reader_versions RED,
apply both owned patches, run one W12 focused GREEN plus affected snapshot
unit guards and root no-run. Do not run the W6 branch or repeat native renders.
W6 currently owns the machine; W12 has no heavy job, queue ticket or request.

Generic unsupported-effect diagnostics currently append public-payload hex heads.
A new authored fixture requires equal builtin/revision/length diagnostics for two
different payloads, preserving slot/reason while exposing no payload bytes.
`effect-diagnostic-privacy-source.patch` removes the hex head and parameter debug
payload; it is pending baseline RED in the same future W12 turn. No native read
is required. Cached SendLevels lengths/list counts remain UNKNOWN; the v0x50
revision itself is not excluded by the strict two-list reader.

NEXT: consolidated W12 reader/privacy RED→GREEN at the next direct release.
