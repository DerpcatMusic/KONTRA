# Tester reader versions (2026-10-09)

Base: `ce714f75`, branch `v2/fix-tester-reader-versions`. Scope: tester triage
rows 5, 6, 8 and 13 in `tester-logs-NwxC/TRIAGE.md`. No library data is copied.

The existing RE reports describe the previous codec limits rather than proving
all versions share a layout. The retained native reader branches establish the
following distinctions. The six relevant original PE ranges were checked against
the pinned Kontakt engine SHA256
`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.
The local static-check receipt is
`/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w6-tester-reader-versions/native-branches.json`.
No native program was executed.

| Family | Evidence in the retained engine export | Reader change |
| --- | --- | --- |
| Parameter arrays | `0x140988740`, internal array reader: fixed capacity before v0x13; v0x11 is an explicit branch. Existing `vendor/ni-file/doc/presets/Kontakt/BParamArray.md` documents presence/chunk framing. | Both shared modulation arrays and FX arrays accept the fixed-capacity v0x11 layout. Native physical slot indices survive. Standalone v0x11 arrays reject trailing bytes. |
| Legacy modulation names | `0x140d09b70`/`0x140d09620` select legacy target layout for Internal v0x80/External v0x100. `0x140d09d50` reads legacy target names through `0x140743010`, whose signed negative branch represents absence. Modern names use the unsigned string reader `0x142a78330`. | Only legacy target/assignment names accept the `-1` null sentinel. Parameter identifiers and modern names keep their bounds. Raw chunks remain the lossless representation. Boolean errors identify target invert/shaper or array slot presence. |
| AHDSR | `0x140cffb40`: v0x10 reads six common floats, one byte, then four floats; v0x11 replaces those four floats with four 13-byte records. | v0x10 has exactly 16 opaque tail bytes, v0x11 at least 52. Editing preserves the original version using these disjoint lengths, without changing the public struct or synthesizing metadata. |
| Compact snapshots | `0x140cff070` accepts versions 1,2,3,4. All share the 24-byte public prefix, FX rack, source, two following group flags and modulation racks. Selection byte is absent in v1, present in v2/v3/v4; v4 adds two u32 words. `0x140d0f8e0` corroborates the writer order. Source layouts are already encoded in `BParSrcMode::read`. | Decode versions 1–4 with versioned trailers. Reuse the existing source codec plus two retained group flags instead of guessing fixed source lengths. External capacity follows its own array version/count (32 or explicitly counted 64), independently of snapshot version. |

Native v0x11 array reading also has a context-dependent path that consumes one
additional u32 after every presence byte when serializer context `+0x3c` is set.
That context is unavailable in the current standalone codecs. This patch does not
retry arbitrary alternative layouts. New v0x11 standalone exact-consumption
checks prevent silently treating that variant's trailing metadata as success.

`vendor/ni-file/tests/reader_versions.rs` contains authored fixtures for all four
array versions, high occupied slot indices, nullable internal/external names,
v0x10 AHDSR edit/roundtrip, and two adjacent compact groups for every snapshot
version combined with source v0x100/v0x102/v0x104/v0x106 and sampler/DFD modes.
Each family includes truncation or malformed-state rejection. Existing v4
mode9/64-slot and envelope compatibility tests remain relevant guards.

## Verdict limits

- The exact tester libraries are absent locally. These fixtures verify codecs;
  they do not certify Evolve, Retro Machines, Maverick, Chris Hein or True Strike
  playback.
- Chris Hein's `0xffffffff` name matches the native legacy-null pattern, but the
  tester's serializer version is not in the bundle. Library-level closure awaits
  a rerun.
- Hurdy Gurdy's `Invalid boolean byte 2` cannot be assigned to a field from the
  old logs. Preserve strict validation; the new error context should identify
  the failing field on a rerun. No fabricated byte-2-as-true workaround.
- Row 13 contains historical 0.3.199 failures; v4 support already existed at this
  base. This patch adds missing v1/v3 support and general source boundaries.
- Native source fields beyond the shared codec's supported versions/modes remain
  rejected. New source checks validate known flag bytes; other source values and
  raw slot bytes remain preserved.

Validation is pending the serialized machine handoff. READY requires the targeted
fixture run, existing reader guards and area `cargo test --no-run`, all through
`kontakto-heavy`. Full release/corpus validation belongs to W0's batch gate.
