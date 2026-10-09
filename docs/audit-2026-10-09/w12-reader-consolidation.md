# Consolidated Kontakt reader validation

W6 `v2/fix-tester-reader-versions` (`d372a4f9` fixtures, `ac3b55a7` source)
is superseded by this W12 slice. Do not integrate or validate it separately.
Prerequisites remain `c1359117`, `4fb8bd5f`, and `7bc29b21`.

The former HOLD fixtures `f3519531` and `4b8a8293` now have recorded RED→GREEN
validation. Receipts: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w12-reader-extras-20261009`.

| Area | Established behavior |
| --- | --- |
| Fixed arrays | Revisions 0x10..0x13 preserve high physical slots and reject truncation/invalid presence. The two bounded v11 layouts remain distinct. |
| Modulation | Legacy nullable assignment names retain alignment; invalid booleans remain rejected with target/field context. |
| AHDSR v0x10 | Opaque negative-zero/NaN tail bits roundtrip and survive edits; known parameter validation remains strict. |
| Compact snapshots | Adjacent records cover v1..v4 × source revisions 0x100/102/104/106 × sampler/DFD modes: 32 combinations, with roundtrip and every truncation boundary checked. |
| Existing v4 wavetable snapshots | The established 99-byte mode9 boundary and counted 64-slot external rack remain intact. An existing guard caught a two-byte candidate overread before integration. |
| Unsupported FX diagnostics | Keep builtin, revision, length, physical slot and typed reason; omit public hex heads and parameter debug payloads. |

RED: reader fixtures had 2 passed/2 failed (compact count and Boolean2 context),
privacy failed 1/1. Final GREEN: reader fixtures 4/4, ni-file snapshot guards 5/5,
privacy 1/1. The snapshot translator filter selects one ignored native exploratory
test and executes zero tests; translation is compile-checked by area/root no-run.
Area no-run and plugin/shot-enabled root no-run both pass through the normal
`kontakto-heavy` FIFO. No W6 duplicate implementation or native render was run.

Exact tester-library audio parity remains unverified. SendLevels cached payload
length/list counts remain UNKNOWN; this slice does not change its DSP admission.
No decrypted library data or samples were written.

NEXT: held_key audition validity and independent-family boundary.
