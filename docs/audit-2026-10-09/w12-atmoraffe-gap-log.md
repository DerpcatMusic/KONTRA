# Supplied-library gaps after the AHDSR repair

Metadata-only witness: `kontra-runs/w12-format-revisions/atmoraffe-metadata.json`
and `atmoraffe-slot-identities-green.log`. The two binary programs are v0x80.
Their complete parsed program/group trees contain zero script chunks (ID6), and
translation contains zero KSP behaviors. There is no authored script slot for
this binder to drop. No script-binding repair is warranted by this witness.

The earlier validation's unsupported values 4 and 8 are **entry counts**, not
native object type IDs. They comprise eight enabled instrument-insert FX slots,
two saved-automation-layout gaps and two source-mode notices. Source mode3 is
played with the sampler fallback; native source-law parity is unverified.

| Rank by impact | Library/item path | Program/group/slot | Typed reason | Enabled / bypassed | Impact |
| --- | --- | --- | --- | --- | --- |
| 1: entire item unavailable | `/home/derpcat/.local/share/kontra/libraries/Embertone Atmoraffe/Samples/Giraffemosphere v2.nki` | program0 | LegacyXmlUntranslated | item count1; slot counts unknown | Binary chunk frontend cannot translate legacy XML. |
| 2: sounding FX dropped | `/home/derpcat/.local/share/kontra/libraries/Embertone Atmoraffe/Instruments/` | programs0/1, insert slots below | NotModeled | 8 / 0 | Changes the currently sounding signal. |
| 3: source approximation | same path | programs0/1, group0 | SourceModeSamplerFallback | 2 scopes; slot counts n/a | Mode3 retains PCM playback, native source behavior unknown. |
| 4: saved controls unavailable | same path | programs0/1, program private | SavedAutomationLayoutNotModeled | 2 records; slot counts n/a | Saved automation parity remains unverified. |

This ranks complete-item loss ahead of currently sounding slot loss, followed by
approximation and saved-control gaps, using the same impact distinction as the
corpus ledger. These new supplied items were outside the earlier 834-item Kontakt
census; its cached 222,757/18,054 dropped-target totals are not re-measured here.

| Native builtin | ID/version | Programs and instrument insert slots | Enabled / bypassed |
| --- | --- | --- | --- |
| Surround Panner | 0x1d / 0x70 | program0:slot0, program1:slot0 | 2 / 0 |
| Send Levels | 0x17 / 0x50 | program0:slot7, program1:slot7 | 2 / 0 |
| Phaser (legacy) | 0x14 / 0x50 | program1:slot1 | 1 / 0 |
| Flanger (legacy) | 0x12 / 0x50 | program1:slot2 | 1 / 0 |
| Chorus (legacy) | 0x11 / 0x50 | program1:slot3 | 1 / 0 |
| Twang | 0x23 / 0x50 | program1:slot4 | 1 / 0 |

All eight have saved bypass=false. Native module identities and versions are
measured, but the disposition is that of this W12 branch, not a new W15/merged
DSP gate. No decrypted payload, script text, authored names or sample bodies
are persisted. The probe does not render or decode sample PCM.

Header inventory inspected every unique `.nki` under installed Kontakt/UVI roots
and the supplied library: **784 total, 781 NIS, 2 binary NKS, 1 legacy XML NKS**,
zero I/O errors. Thus legacy XML is 1/784 (0.128%) of this NKI inventory. This is
header classification, not payload validation; `.nkm`, UVI banks and archives
are outside its denominator. The sole legacy header is v0x100. Reader contracts
`BPatchHeader::read_le` and `KontaktV1/V2` establish XML for header versions≤0x10f.
Receipt: `kontra-runs/w12-format-revisions/legacy-header-census.json`.

NEXT: W15 ranks the native FX revisions; W0 retains the XML/source/automation gaps.
