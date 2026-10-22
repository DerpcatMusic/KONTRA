# Kontakt 8 voice, group and zone engine: spec and gap table

Status: **partial, waiting on the Kontakt 8 decompile export** (`source/kontakt8/READY`,
not present at 2026-10-21 19:13 local; Ghidra analysis still running). No new DSP
law is claimed here. Everything below is either a repo-recorded binary result, a
documented behaviour, or an explicit open item. Nothing is copied from decompiled text.

Sources read: [KONTAKT_VOICE_GROUP_DEFAULTS.md](../KONTAKT_VOICE_GROUP_DEFAULTS.md),
[KONTAKT_ALLOCATION_M1.md](../KONTAKT_ALLOCATION_M1.md),
[KONTAKT_START_CONDITIONS.md](../KONTAKT_START_CONDITIONS.md),
[KONTAKT_TOPOLOGY_EVIDENCE.md](../KONTAKT_TOPOLOGY_EVIDENCE.md) (B13, T19),
`crates/sampler-core/src/{steal,native_start,release,blocks/glide}.rs`,
`crates/sampler-ir/src/lib.rs` (Group, VoiceLimit, Trigger, Sequence, GroupStart),
`crates/sampler-kontakt/src/{library,keyswitch}.rs`.

Public manual: `docs.native-instruments.com` returned **HTTP 403** to the fetch tool
on this run, so the manual wording below is taken from the copies already quoted in
the repo docs (checked 2026-10-13). Re-verify the Voice Groups and Group Start Options
pages once access works.

## Module 1: voice groups, polyphony and stealing

| Parameter | Native field (repo evidence) | Unit / range | Default | Engine status |
|---|---|---|---|---|
| Max voices | 32-bit, `+0x28` (B13 reader `0x140d049b0`) | voices, ≥1 | 1 | Mapped (`VoiceLimit.voices`) |
| Stealing mode (kill) | 16-bit, `+0x2c` | enum 0..4 = Any/Oldest/Newest/Highest/Lowest | 1 (Oldest) | Mapped; all five kill modes implemented in `enforce_voice_limits` |
| Released-voice preference | byte, `+0x30` | bool | nonzero | Mapped (`prefer_released`); see open item O1 |
| Fade time | 32-bit, `+0x34` | ms | 10 | Mapped (`fade`); fade DSP law for stolen voices is the existing `Stealing` fade, not native-measured |
| Exclusion group | 32-bit, `+0x38` | set 0..15, -1 none | -1 | Mapped (`exclusive_set`); cut semantics implemented in `cut_exclusive_domains` |
| Voice-group index | 128 physical groups, mask indexed `group>>3`, bit `group&7` | index | unassigned | Default materialised only when a sample group assigns it (see KONTAKT_VOICE_GROUP_DEFAULTS.md) |

Behaviour implemented (shared engine, `crates/sampler-core/src/steal.rs`):

- Limits apply per domain: instrument-wide, voice group, and an independent source
  (Time Machine) budget. Counts exclude a stolen voice immediately; the fading voice
  stays in the list until its fade ends.
- Exclusive sets: a new voice cuts every sounding voice in *other* domains of the same
  set (a domain never excludes itself), each with the victim domain's fade.
- Monophonic release triggers: a repeated key in a release group cuts all previous
  release voices of the same key (all layers, all plan generations), fade = rate/100.
- Victim order when over limit: released-preferred pool first if `prefer_released`,
  then the kill mode over that pool.

Gap / open:

| ID | Item | Status | Needed to close |
|---|---|---|---|
| O1 | Native victim algorithm for Any (quietest) and the Pref.Rel rule. The code keeps the pre-M1 law and says so; the manual's released-note priority differs from the code. | **Open.** Topology T19; B13 proves field identity only. | Decompile of BVoiceLimit slot 8/9 (`0x140d049b0`, `0x140d13370`) and a labelled native probe. |
| O2 | Exclusive-set fade (the code reuses the victim domain's fade, a v1 policy). | **Open**, no native duration. | Native probe or decompiled cut routine. |
| O3 | Monophonic release fade (10 ms, `rate/100`). | **Open**, manual specifies the cut, not the duration. | Same as O2. |
| O4 | Headroom: `Stealing::for_limits` uses voices/4 fade slots. | Engine choice, not a Kontakt value. Keep as an engine setting; no native claim. | None; document as engine default. |

## Module 2: round robin and group start criteria

Binary-proved (KONTRA_START_CONDITIONS.md, Kontakt 8.13.1 generator `0x140876bd0`):

- Mode 3 = cycle round robin. One counter per generated instrument context, starts at -1,
  incremented on each ordinary note before group predicates are tested.
- Stored position is 0-based. The cycle length is max stored position + 1. Duplicate
  positions match together; missing positions consume silent steps; no eligibility filter.
- Release-generated events reuse the index instead of incrementing.
- Mode 4 = random: one draw per ordinary callback; ordinal per random row.
- Joins: `0` AND, `1` AND NOT, `2` OR (reader/writer identity). Mixed-operator precedence **unproved**.

Current engine comparison:

| Behaviour | Engine (`native_start.rs`, `keyswitch.rs`) | Match |
|---|---|---|
| Stored position 0 is first step | Mapped as `cycle_class + 1`; evaluator tests `tick % len + 1 == position` | Yes |
| Cycle length = max stored + 1 | `native_rr_length = max(position)` where position = stored + 1 | Yes |
| Counter increments on ordinary notes only | `lib.rs` note-on path increments `native_cycle`; release selections take the attack's captured tick (`selection.rs` snapshot) | Yes (by reading; not run against a native trace) |
| Random draw, PRNG | SplitMix64 over `native_seed` and tick | **No evidence.** Native PRNG and seed are unproved; keep, do not claim parity |
| Mixed join precedence | Left fold | **Unproved**, keep, do not claim parity |
| Slice mode (5) | Not mapped (diagnosed) | Open; legacy beat-sequencer timing unproved |

## Module 3: triggers and onset rules

| Trigger | Engine (`sampler-ir` `Trigger`) | Kontakt mapping |
|---|---|---|
| Attack | Yes | Default for zones (`library.rs:3294`) |
| Release (key/gate) | `KeyRelease`, `GateRelease`; release groups use `GateRelease` (`library.rs:3292`) | Mapped from the group release flag |
| First / Legato | `First`, `Legato` implemented in the IR and selection | **Not mapped from Kontakt.** No decoded zone field found in repo docs; open |
| Transition (interval) | Engine `Transition { low, high }` | Test-only mapping (`cache_zones.rs:441`); no native field decoded |

Open: which Kontakt zone/group field selects First or Legato, and the exact onset rule
when a legato note overlaps a held one. Needs the decompiled zone trigger reader.

## Module 4: glide and legato pitch

- Engine block: `crates/sampler-core/src/blocks/glide.rs` (`GlideState`, `Glide { time, legato_only }`,
  per-semitone or fixed time). Tested for legato-only and constant rate.
- Kontakt: the Glide modulator (type `0x0B`) is **decoded but diagnosed as unsupported**
  (`library.rs:1876-1895`). Missing: the caller that maps the note pitch and time to the
  ramp start, step and gain (`0x140ceb090` setup).

Gap: no Kontakt glide law. Do not map the modulator until the setup routine is decoded.

## Module 5: choke, exclusion and group start options (gap summary)

| Item | State |
|---|---|
| Choke by exclusion group | Implemented in engine; native fade open (O2) |
| Group start options (RR, key, controller, always) | Implemented; RR matches binary; random and join precedence open |
| Zone/group key and velocity switching | Covered by existing selection and predicate code; not re-audited here |
| Release trigger monophonic | Implemented; fade open (O3) |

## Evidence confidence

- High: field layout and default values of voice limits (B13, decoded by identity and
  instruction reads, same executable hash as in the repo docs).
- High: round-robin counter and stored-position law (generator and matcher addresses).
- Medium: engine behaviour matches those laws by code reading. No native trace was run.
- Low / open: victim order for Any and Pref.Rel, exclusion and monophonic fades, random
  PRNG, join precedence, First/Legato mapping, glide law.

## Next steps when READY exists

1. Search the export for `BVoiceLimit` (slots 8 and 9, `0x140d049b0`, `0x140d13370`)
   and the voice-steal routine. Record the victim order and fade formula as this doc's
   own spec. Close O1 to O3.
2. Find the zone trigger reader for First and Legato. Map them in `sampler-kontakt` if found.
3. Trace the random draw and join evaluation only as far as needed for the PRNG and
   precedence questions. Do not claim parity without a native vector.
4. Re-check the public voice-group and group-start manual pages once the 403 clears.

## Revision 2026-10-21 (no decompile): what changed and what is measurable

- **Refactor.** `sampler_core::Kill` was a second copy of `sampler_ir::Kill`, mapped arm by
  arm in `lower.rs`. It is now a re-export of the IR enum, so the victim law has one
  configuration type. Behaviour is unchanged; the M1 victim, exclusion, monophonic and
  choke tests pass (`lower m1_`, `steal`, `choke`, `release_selection`).
- **Wine oracle.** `tools/kontakt-oracle/render.py` can record Kontakt's audio output
  (PipeWire null sink) for a probe that plays notes from a KSP timer. `audit_probes.py`
  still labels runtime voice/steal/trigger rows BLOCKED because the state oracle cannot
  deliver MIDI. Measuring O1–O3 (victim order, exclusion and monophonic fades) needs a
  labelled multi-note render through this path. It was not run in this revision: the
  harness drives Kontakt by mouse clicks and writes scratch under `/tmp`.

## Revision: decompile READY (2026-10-21, evening)

- Confirmed in `source/kontakt8/0140d00000.c` (`FUN_140d049b0`, 0x140d049b0): version 0x60
  reader stores the 16-bit kill mode at +0x2c, the preference byte at +0x30, and three
  32-bit values at +0x28, +0x34, +0x38, as recorded above. Writer `FUN_140d13370` mirrors
  the same order.
- Not yet located: the victim routine (O1), the exclusion cut and the monophonic fade
  (O2, O3). Decompiled functions carry no recovered names (`FUN_*`); the field offsets are
  too generic to find the allocator by text search. The next step is a read-only
  cross-reference from these reader/writer addresses to the voice manager's allocation
  code, not more string search.
- Rows O1–O3 therefore stay open. Nothing in this revision changes engine behaviour.

## Revision: victim routine located (decompile, 2026-10-21)

Source: `source/kontakt8/0140860000.c`, `FUN_14086fea0` (size 657). Read as data; the
spec below is our own wording.

- **Candidates.** Walk the engine's voice list (head at engine +0x11ed0, next at voice
  +0x920). A voice is a candidate when its state byte (+0x184) has no bits outside 0x02,
  i.e. state 0 or 2; the new voice itself is skipped.
- **Scope.** Three generation/domain matches gate the walk: +0x8d4 (plan generation),
  +0x8bc (voice-group domain), +0x8dc (source domain). Which flag selects which match
  follows from the call sites; the engine's `enforce_voice_limits` already keys its
  domains the same way.
- **Kill selectors** (`limit +0x2c`, 16-bit):
  | Mode | Key field | Rule |
  |---|---|---|
  | 0 Any | +0x51c | maximum, initial 0 |
  | 1 Oldest | +0x534 | maximum, initial -1 |
  | 2 Newest | +0x534 | minimum, initial INT_MAX |
  | 3 Highest | +0x518 (note key) | maximum, initial -1 |
  | 4 Lowest | +0x518 (note key) | minimum, initial 128 |
  Oldest/Newest/Highest/Lowest agree with the engine's ordering (larger +0x534 = older).
  Tie-breaking in the native walk is first-in-list at strict comparison; the engine breaks
  ties by birth order. **Unverified** for ties.
- **Any is different from the engine.** The engine ranks Any by gain (quietest). Native
  ranks by +0x51c, an integer that `FUN_14099c140` computes when a voice starts from two
  engine counters and a float ratio, clamped to at least 1. Its musical meaning is not
  yet established; it is not a level. Parity needs a native probe.
- **Prefer-released.** If the limit's prefer byte (+0x30) is set, the same key rule is
  first applied to candidates whose +0x504 byte is 0; if any exist, the victim comes from
  that set. Inferred polarity: +0x504 is set to 1 at voice start (`FUN_14096a380`) and
  cleared on the release path (`0140af0000.c:7356`), so 0 = releasing. Matches the engine.
- **Cut.** The victim goes to `FUN_14086d320`: state 1 (fading), fade length float
  stored at +0x53c, and an immediate state 3 when the owner lacks flag 0x1000. The float
  argument at the call site is decompiled as a pointer; the native fade value is not
  recovered yet, so the engine's ms fade stays unverified.
- **Per-key record.** After a cut, a u16 0x100 is written into a per-key table at engine
  +0x27d05 (and +0x27e05) indexed by the victim's note key. Purpose not recovered.

Matrix impact: the voice row is `partial` (ordering matches for four kill modes; Any key,
tie-breaks, fade law and per-key record are open).

## Revision: field meanings from writers (decompile, 2026-10-22)

- **+0x534 (Oldest/Newest key).** Constructed as -1 (`FUN_1405dc150`, `01405d0000.c:9587`)
  and reset to 0 when a voice is (re)started (`FUN_1408d9d60`, `01408d0000.c:7777`). No
  per-voice stamp is written elsewhere. It therefore reads as an elapsed count since voice
  start, not a global birth index. Oldest (max) and Newest (min) orderings then equal the
  engine's birth-order ordering except for voices started in the same block, which tie
  natively. The tie order is list order and is not recovered.
- **+0x51c (Any key).** Written by two routines: `FUN_14099c140` (`0140990000.c:9968`)
  computes it at setup as `max(1, round(a * k * ratio / b + c))` from two integer counters
  of the sample object and a float ratio; `FUN_140968ba0` (`0140960000.c:4944`) stores a
  script-supplied value into voices matching a group/channel filter. Because the first
  writer runs on the sample object, the Any key is a voice-length or priority quantity,
  not a loudness. The engine's gain ranking for Any is therefore not the native law. The
  exact meaning of the two counters is not recovered.
- **+0x518 (Highest/Lowest key).** Integer, compared directly. Not written in the voice
  start path that was read, so its source remains open.

Matrix: unchanged, still partial. Any and the steal fade law stay open.

## Revision: steal fade and the cut call (decompile, 2026-10-22)

- The victim walk ends in `FUN_14086d320(victim, 0, <float>, flag, 0)` (`0140860000.c:9622`).
  The third argument is the limit object in the walk's own signature, but the callee takes a
  float there. The decompiler shows no conversion, so the native fade is not recoverable from
  this site. It is not the limit's +0x34 field as written.
- The other call sites pass a per-voice float (`FUN_14084f380`, `0140840000.c:9586`, from
  voice field +0x264) and a constant (`0140a70000.c:5312`). Neither is the victim-limit fade.
- Conclusion: the steal fade law (O2/O3 and the fade part of O1) needs a native measurement.
  The Wine oracle is the path: a probe that plays overlapping notes under a one-voice limit and
  records which note is cut and over how many milliseconds. A Kontakt Wine session was already
  running on this machine during this pass (`start.exe`, `explorer.exe`), and the oracle has a
  one-session guard, so no oracle run was started.
