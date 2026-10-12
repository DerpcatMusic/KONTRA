# W12 bounded NICNT directory indexing

Vista's NICNT has a complete directory followed by a physically short member.
The old resource loader rejected the entire directory at the final payload
extent check, preventing it from answering whether a requested picture was
listed. `0cb7a8a0:vendor/ni-file/src/file_container/mod.rs` already indexed
metadata independently of payload availability. This port retains v2's marker,
count, range and overflow checks while separating directory indexing from
strict full-container and per-member reads.

`NIFileContainer::read` still rejects incomplete NKI/NKM monoliths.
`read_index` validates a complete directory; `read_member` checks physical EOF,
overflow and allocation limits before reading. `ResourceContainer` uses that
index and the shared member reader, preserving its typed 32 MiB limit error.
No per-library name substitution, image synthesis or renderer change is added.

The authored fixture first validates a complete two-member container. RED then
rejects its complete directory when only the second payload is shortened.
GREEN indexes both names, reads the intact first member and rejects the short
second member. Every incomplete directory prefix remains rejected; strict
full-container reads still reject every incomplete payload prefix. A separate
resource-route RED/GREEN fixture proves that absent names return absence while
a listed, short member returns an error. Reader and Kontakt area no-run checks
pass; the final resource check retains the original typed limit guard.

Native file SHA-256:
`4f56d68027d91b5c9d81b43211f25e58dd5e4d6daf58f882584b3a4c76e26007`.
The file is 514346 bytes. Its real resource metadata starts at 512064, lists
one member and declares 54344 payload bytes. Both TOC markers and the end
marker validate; the payload begins at 514240 with only 106 bytes remaining.
The marker at zero belongs to product metadata, not a valid resource table;
the earlier offset-zero source lead is excluded.

The product reader now indexes that one native member. It remains unreadable.
Neither `Resources/pictures/pic.png` nor
`Resources/pictures/admin_bg.png` is listed; both return absence. This closes
the previously rejected-directory membership question without relaxing member
read validation or inventing missing bytes.

W3's fresh exact-request witness and frozen-v1 pair cover Pacific 10 Cellos
Trills and Vista 3 Violins FFF Overlay. Both request `pic` on a visible Switch
and `admin_bg` on a hidden Label. All located pictures decode successfully
(Pacific 4/4, Vista 3/3); frozen v1 reports missing images for both exact NKIs.
The complete Kontakt-folder search finds zero exact, basename, stem, case or
double-extension matches in 20 indexed containers plus Vista's bounded name
table. With Vista's table now parsed through the product reader, these two
requests are absent from the supplied library data. Under the user's explicit
absent-member disposition, classify them as by-design/absent authored requests
and stop the image-fix queue. There is no image-loading or playback gain.

Receipts: `w12-nicnt-index-20261010/{READY,NATIVE,W3_HANDOFF}.json` and
`w12-impulse-nkr-20261010/VISTA_NICNT_STRUCTURE.json`. Native payloads and other
member names stay in memory. A full current UI gate, timing and native audio
parity are unmeasured; W0 owns the integration gate.

NEXT: retain the truncated Vista member as a data-extent issue; resume the
remaining Areia impulse-table and native family/DSP gap census.
