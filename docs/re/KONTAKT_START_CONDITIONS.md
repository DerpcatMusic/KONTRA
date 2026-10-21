# Kontakt 8.13.1 group start conditions

## Scope and confidence

Binary SHA-256: `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.

Static extraction from `/mnt/Windows11/DEV_WORKSPACE/re-binaries/Kontakt8.exe`, 2026-10-13. All addresses below are image virtual addresses (base `0x140000000`). No source changes, builds, live Kontakt sessions, or decrypted library extraction. This report describes the built-in generated-system-script path. User-script suppression and callback admission are separate boundaries.

**Main evidence:** `0x140876bd0` compiles group-start configuration into KSP source. `0x1409723a0` calls it and installs/compiles the instrument's script contexts. This is not merely a serializer. Its strings explicitly form `on init` (`0x144f18be8`) and `on note` (`0x144f18c08`) callbacks. The group-start block is wrapped in `USE_CODE_IF_NOT(NO_SYS_SCRIPT_GROUP_START)` (`0x144f17cf8`). Behavior when that condition disables the built-in block must not be guessed.

## Stored fields and API names

`BParStartCriteria` is SerType `0x0f`, version `0x70`, reader `0x140d04400`, writer `0x140d12ec0`. Vtable RVA `0x4e96fc8` means VA `0x144e96fc8`, **not a file offset**. Constructor `0x1407f8eb0` sets the vptr; it does not prove defaults.

The four runtime records begin at group `+0x790`, stride `0x30`. Reader offsets below are relative to a standalone record; runtime offsets are relative to the group for slot zero.

| Standalone | Runtime | Type | Meaning | Lua API ID/name |
|---|---|---|---|---|
| `+0x08` | `+0x798` | u32 | mode 0..5 | `0x324 mode` |
| `+0x0c` | `+0x79c` | u16 | key minimum | `0x325 key_min` |
| `+0x0e` | `+0x79e` | u16 | key maximum | `0x326 key_max` |
| `+0x10` | `+0x7a0` | u16 | controller number | `0x327 controller` |
| `+0x12` | `+0x7a2` | u16 | controller minimum | `0x328 cc_min` |
| `+0x14` | `+0x7a4` | u16 | controller maximum | `0x329 cc_max` |
| **`+0x18`** | **`+0x7a8`** | u32 | **zone index** | `0x32b zone` |
| **`+0x1c`** | **`+0x7ac`** | u32 | **slice index** | `0x32c slice` |
| **`+0x20`** | **`+0x7b0`** | byte | **internal/sequence-only flag** | `0x32d internal` |
| `+0x24` | `+0x7b4` | u32 | authored RR position | `0x32a position` |
| `+0x28` | `+0x7b8` | u32 | connective to later condition, 0..2 | `0x32e next` |

Addresses: Lua wrapper getter `0x1410c4a70`, setter `0x1410c4850`; implementations `0x1410c66a0`, `0x1410c5a10`; generic field getter `0x140898570`, with start-field cases at `0x14089dd04` onward. String-view field entry `0x14559fac8` is **internal**, not round_robin (the unrelated following string starts at `0x14559fad8`). Official KSP names for the three previously unknown fields are `ZONE_IDX`, `SLICE_IDX`, `SEQ_ONLY`.

The position setter stores its argument unchanged; the public getter reports stored value **plus one**. Do not apply that getter conversion again to stored positions.

## Round robin: binary-proved selection law

Mode `3` is Cycle Round Robin. `0x140876bd0` scans every group and all four fixed slots, taking the signed maximum of `stored_position + 1` for mode-3 records. There is no per-position counter and no partition by `cycle_class`.

- Cycle length: **maximum stored position + 1**, not number of RR groups.
- Shared state: **one `$curIdxRoundRobin` per generated instrument context**, not per group, key, RR position, or MIDI channel.
- Initial state: **-1** (`0x144f17e70`).
- On a note callback with `$NI_SYS_EVENT_PAR_3 == 0`: increment, then reduce modulo cycle length (`0x144f17ea0`).
- Match: current index equals **stored position** (`0x144f180b8`). Thus stored **0 is the first matching position**, and the first ordinary callback selects 0.
- Duplicate positions match together. Missing positions consume silent steps; no gap compaction or eligibility filtering occurs.
- The increment happens **before group predicates**, not after an audible group wins. Failed controller/key tests do not stop it once this code runs. Mapping success is not checked in the generated increment.
- Release-generated note events carry `$NI_SYS_EVENT_PAR_3=1` (`0x144f17920`); they **reuse**, rather than increment, the RR index. This is not the same as saying MIDI note-off advances RR.

The scan considers all four mode fields directly, including nonzero records beyond a zero-mode gap. It does not consult the serialization occupied mask. Reset/reload behavior beyond `on init` and callbacks never reaching this block is **unproved**.

## Random: binary-proved algorithm, PRNG boundary

Mode `4` is Cycle Random. `0x140876bd0` sets `N` to the **number of mode-4 records across all groups and all four slots**. It does not count eligible/mapped groups, deduplicate groups, or use authored RR positions.

Before evaluating any group, the callback executes exactly one selection expression:

`$curIdxRandom := random(0, $numRandom - 1)`

Template `0x144f17fa8`, count declaration `0x144f17f68`. Each mode-4 condition is compared with a sequential compiler-assigned ordinal (`0x144f180d0`): instrument group order, then slot order, starting at zero. A group containing two random rows has two distinct ordinals. AND of those two rows cannot pass on the same draw; OR can.

No generator-level last-winner memory, reroll, shuffle bag, or no-repeat check exists. The random draw is **outside the RR release guard**: a release-generated note callback that reaches this block makes a new random selection. Failed predicates do not cause a second draw. Raw count includes unreachable/unusable rows; unused ordinals can therefore produce no admitted group.

**Unproved:** the exact KSP PRNG, seed, distribution/rejection mapping, and random-state sharing with user scripts. The similarly named Lua `math.random` implementation `0x141e63430` is a different runtime and is **not evidence** for this KSP call. Do not substitute SplitMix64 or claim bit-exact native randomness. NI documents inclusive `random(min,max)` bounds.

## Condition chaining

Mode IDs: `0 Always/none`, `1 Key`, `2 Controller`, `3 RR`, `4 Random`, `5 Slice` (reader `0x140d04400`, generator `0x140876bd0`).

For ordinary populated rows, conditions are emitted in slot order. The compiler scans ahead for later nonzero modes when deciding whether to emit the current row's connective. Terminal mode-0 rows do not emit predicates. Sparse/interior-zero configurations are not validated here; do not infer a general gap-compaction rule. The compiler emits an outer `if(not (...))` and disallows the group on failure (`0x144f17fd8`, `0x144f18234`, `0x144f18238`). It parenthesizes each predicate but **does not build an explicit left-fold parenthesized expression**.

| Stored `next` | Generated operator | Address |
|---|---|---|
| 0 | `and` | `0x144f18208` |
| 1 | `and not` | `0x144f18218` |
| 2 | `or` | `0x144f18228` |

`AND NOT` negates the next predicate using the KSP unary `not`; it is not an independent condition mode. There is no operator emitted if there is no later nonzero condition. An all-Always group needs no condition expression unless the legacy beat sequencer exists, when the ordinary-note guard is emitted. The official manual treats terminal Always as a marker/no-op, not a truth-valued row to preserve in a generic left-fold implementation.

KSP parser `0x1419f4550` constructs unary NOT through `0x1407afcd0` (AST kind `0x2e`), AND/OR through `0x1407a73d0` (kinds `0x2f/0x30`). **Unproved: precedence and associativity of mixed AND/OR chains.** The official Boolean documentation gives truth tables only. The parser tables were located but no mixed-expression reduction trace was completed. Do not assume left-to-right folding or AND-before-OR from the generator alone. Pure AND chains, pure OR chains, and a two-row AND NOT follow their ordinary truth tables; mixed examples such as `A or B and C` need a parser-table trace.

## Slice trigger and fields +0x18/+0x1c/+0x20

Mode `5` uses zone/slice identity in **internal event parameters**, not MIDI note comparison:

- `internal != 0` (`SEQ_ONLY`): require event parameter 1 = `zone + 1000` **and** event parameter 2 = `slice + 1000` (template `0x144f180f0`). Ordinary untagged notes do not pass this predicate.
- `internal == 0`: admit **any event with `$EVENT_NOTE >= 0` OR the same matching tagged zone/slice event** (template `0x144f18170`). This does not mean “ignore slice identity.” Ordinary mapped notes remain playable.
- Neither branch uses `+0x24` for slice selection.

Legacy internal beat-sequencer groups are identified by group `+0xba8 == 5` and `+0xc38 == 0`. The generator emits a disallow for the source group (`0x144f18570`), then creates slice events with `play_note(-1,-1,0,_slice_length(...))`; parameter 1 is `1000 + zone_index`, parameter 2 is `1000 + $sliceIdx` (`0x144f18810`). Slice index starts at 0, advances after a length-based wait, and follows legacy loop/count controls (`0x144f185d0`, `0x144f18720`, `0x144f189f0`).

Zone resolver `0x14087ff60`, called at `0x140878540`, searches the selected instrument's zone vector (`+0x11340..+0x11348`) for zone `+0xa20 == group_index`. It prefers a matching record with nonzero byte `+0x7b4`; otherwise it returns the first matching zone, or -1. That byte belongs to a **zone**, not the criterion's RR field with the same numeric offset. Its returned index is inserted into event parameter 1, not into `_slice_length`.

If any internal beat-sequencer group exists, the compiler adds `$EVENT_NOTE >= 0 and $EVENT_VELOCITY >= 0` to ordinary Key, Controller, RR, Random predicates (`0x144f18038`, `0x144f18088`). This keeps synthetic negative-note slice events from passing those atoms, while Slice mode explicitly handles their tags. It does **not** guard the earlier RR increment or random draw.

The NI manual calls Slice Trigger legacy Kontakt-2 compatibility behavior and advises against explicit use. Full beat-machine timing units, stop/reset behavior, and how tags survive other user scripts are **unproved** here.

## Key/controller context

Controller mode emits inclusive `in_range(%CC[number],minimum,maximum)` (`0x144f18018`). Key mode tests a latched range flag, not the current note directly (`0x144f18070`). Unique key ranges are collected across all four slots; defaults derive from instrument `+0x21b94`. A note inside the overall min/max span clears all range flags, then sets flags for each containing range (`0x144f17d28..0x144f17dc0`). A note in a gap inside that span clears every flag; a note outside the span preserves the latch.

## Implications for the current evaluator (no code changes)

`crates/sampler-core/src/native_start.rs` currently rejects position 0, uses a one-based comparison, counts distinct random groups, generates SplitMix64 values, and left-folds joins. These choices are **not established native fidelity**. The stored-zero RR and per-random-row ordinal findings directly contradict the corresponding assumptions. Mixed-join equivalence and PRNG equivalence must not be claimed without the evidence below/further extraction.

## Sources and reproducibility

- Queries: `tools/re/query.sh '@0xADDR' 0`, read-only Ghidra; raw PE address mapping and focused radare2 disassembly used as cross-checks. Private project `/home/derpcat/kontakto-start-research-ghidra` was queried serially.
- Saved evidence: `start-evidence/generator.txt`, `system-script-templates.json`, `api-wrappers.txt`, `api-fields.txt`, `api-generic-getter.txt`, `api-offset-disassembly.txt`, `generator-installer.txt`, `installer-and-slice-helper.txt`, `ksp-parser.txt` next to this spec.
- Official [Kontakt Group Start Options](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view#group-start-options).
- Official [KSP Group Start Options parameters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters#group-start-options) exposes `CYCLE_CLASS`, `ZONE_IDX`, `SLICE_IDX`, `SEQ_ONLY`, `NEXT_CRIT` (getter-only constants).
- Documentation notes: `start-docs-evidence.md` in this directory. Repository oracle `docs/re/NATIVE_ORACLE.md` records getter stored+1 behavior; this report's stored-zero runtime conclusion comes from the native generator, not that getter alone.

## Explicit remaining gaps

1. Exact KSP random PRNG/seed and bit-exact draw mapping.
2. Full callback admission: ignored/unmapped notes, user-script order/suppression, and whether every external MIDI event reaches this block.
3. Reset/recompile persistence, saturation/overflow and invalid authored values.
4. Mixed Boolean precedence/associativity and short-circuit behavior.
5. Full legacy beat-machine sequencing/timing and external event-tag mutation.

No assumptions or constants were changed in implementation. Binary-proved rules above apply when the built-in generated block executes; unresolved boundaries are not approximated.
