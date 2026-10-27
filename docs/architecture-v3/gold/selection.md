# Gold standard: which samples a note plays (selection)

Status: research spec, no code changed, nothing built. Evidence tags:
`[source]` vendor-neutral open source read in `Research/samplers`, `[doc]` manual or spec text,
`[binary]` read from a vendor binary (RE docs or strings), `[lib]` read from a library file,
`[inferred]` my reading, not measured. KONTRA references are file:line in this worktree.

## 1. Summary

- Selection is one decision per note: which zones start (key, velocity, CC, switch, cycle, trigger), with what
  fades and offsets. Every format answers it with different primitives. KONTRA's IR cannot express most of
  Kontakt's answer, and its engine applies some laws (fade curve, random draw) that differ from Kontakt.
- Kontakt's start options are a boolean list (AND, AND NOT, OR). KONTRA's IR `StartRule` is AND-only
  (`crates/model/ir/src/lib.rs:234-247`). OR is diagnosed then ANDed (`translate.rs:266-272`); AND NOT is not
  even diagnosed (`translate.rs:262-265` tests only `next == 2`).
- Velocity/key crossfades: Kontakt is a measured linear ramp `(v-L+1)/(F+1)` [doc, KONTAKT_REFERENCE s.11, s.15].
  KONTRA uses equal power, `sin/cos` (`plan.rs:77-80`), and drops key crossfades entirely (`translate.rs:360-362`).
  sfizz defaults to equal power and offers `gain` (linear) [source].
- Round robin: all four references use a counter. They differ in scope (Kontakt: one per instrument; HISE:
  one per sampler; SFZ: one per region) and in when it steps (Kontakt: every note-on callback, before any
  predicate; SFZ: only on notes inside the region's key range). KONTRA steps only if a zone passed every other
  predicate (`lib.rs:393-411`).
- Random: Kontakt draws once per note over `N = number of random rows`, with no no-repeat rule. KONTRA counts
  groups, not rows (`translate.rs:255-258`) and applies a no-repeat reroll (`behaviour.rs:126-130`).
- Keyswitches: Kontakt silences only switch keys. KONTRA silences every key inside the span between the
  lowest and highest switch range (`prepare.rs:37-42`, `lib.rs:353-358`, `lib.rs:389-391`).
- Release triggers: the Kontakt time-counter attenuation is not translated (`prepare.rs:250` leaves
  `release_decay_db` at 0; no producer in `crates/format`), and the engine law is exponential dB/s
  (`lib.rs:416-418`) where the recovered Kontakt law is linear (`docs/re/engine/BASIC_BEHAVIORS.md` row 6).
- The random sample-start range (Kontakt `sample_start_mod_range`) is read into IR and used only for the preload
  window (`prepare.rs:164-188`); playback never applies it. The Kontakt law is not recovered
  (`docs/re/engine/UNROUTED_START_RANGE.md`).

## 2. Reference behaviour

### 2.1 Kontakt 8

Start options (group start conditions):

- Modes: Always, Start on Key, Start on Controller, Cycle Round Robin, Cycle Random, Slice Trigger
  [doc: KONTAKT_Manual.txt ~6607-6630, "Group Start Options"]. Slice Trigger is legacy and "should not be used".
- Conditions are a list joined by per-row operators: AND, AND NOT, OR. "Multiple Group start conditions, combined
  with 'and' operators. This means that all specified conditions must be met" [doc: manual ~6598].
  The KSP enum confirms `$START_CRITERIA_AND_NEXT`, `_AND_NOT_NEXT`, `_OR_NEXT` (KSP_Manual.txt ~12795-12820).
- Stored operator values 0 AND, 1 AND NOT, 2 OR [binary: `docs/re/KONTAKT_START_CONDITIONS.md`, Condition chaining,
  addresses `0x144f18208`, `0x144f18218`, `0x144f18228`]. Mixed AND/OR precedence and associativity: unproved
  (same doc, "Unproved: precedence and associativity").
- Start on Key: a group is inactive until a note in its key range is received, and deactivated when another
  Start-on-Key group becomes active [doc: manual ~6611-6614]. The binary's latch: a note inside the overall
  span clears all range flags, then sets the flags of ranges that contain it; a note in a gap clears every flag
  (`KONTAKT_START_CONDITIONS.md`, Key/controller context) [binary].
- Round robin: one `$curIdxRoundRobin` per generated instrument context, initialised to -1; incremented on each
  note-on before group predicates; cycle length = max stored position + 1; duplicate positions match together;
  missing positions consume silent steps; release-generated events reuse the index [binary: KONTAKT_START_CONDITIONS
  Round robin section, `0x144f17ea0`, `0x144f180b8`, `0x144f17920`]. This is also the manual's model: "cycled in a
  round-robin fashion on each note" [doc: ~6618].
- Random: exactly one draw `random(0, N-1)` per note-on callback, N = number of mode-4 rows over all groups and
  all four slots, before any group is evaluated; row ordinal in group order then slot order; no no-repeat, reroll
  or shuffle bag [binary: KONTAKT_START_CONDITIONS Random section, `0x144f17fa8`, `0x144f17f68`, `0x144f180d0`].
  The draw is MT19937 with a Lemire bounded map [binary: `0x1410ebf10`, `0x1414e6140`]. The seed is
  `std::random_device`, so there is no fixed native seed (same doc, Open 1).
- Release triggers: fire on note-off; with a sustain pedal down they wait until pedal-up [doc: manual ~8518-8524;
  binary: row 26 in BASIC_BEHAVIORS]. Monophonic release: a new release cuts earlier release samples of the same
  note with a 10 ms fade [doc: manual ~8532-8535; binary row 7].
- Release counter (t ms): the timer counts back from t at note-on and stops at note-off; its value is a modulation
  source [doc: manual ~8526-8531, and "Release Trigger Counter" source ~13880]. Recovered law:
  `max(T - X, 0) / T` in ms, with T the group counter [binary: BASIC_BEHAVIORS row 6, `0x14084f380`]. The engine
  uses this as a modulator, not a fixed dB drop.
- Zone crossfades: key and velocity only; no CC crossfade [doc: manual ~7077-7130]. Measured curve: linear
  amplitude gain `(v - L + 1)/(F + 1)` on the fade-in, mirror `(H - v + 1)/(F + 1)` on the fade-out, first step
  `1/(F+1)`, not equal power, not dB-linear [measured: KONTRA docs/architecture-v2/KONTAKT_REFERENCE.md s.11,
  example at L=30 F=60 v=40: 0.180 measured, sqrt law would give 0.42]. Key crossfade: same formula, measured
  within about 1 dB (s.15, KONTAKT_REFERENCE line ~218). The curve is "Not stated" in the manual (s.Not stated row).
- Legato (group button): on a legato phrase, playback position carries over from the previous note [doc: manual
  ~8746-8748]. Melody engine: Off / On / Legato (take over only if the previous key is still held) [doc: ~3775-3781].
- Legato intervals and previous-key transitions: the manual's glide section discusses legato pitch transitions
  (~13816), but this study did not verify how Kontakt 8 stores legato intervals. KONTRA's old crate handled
  `Transition` (BASIC_BEHAVIORS row 9), so Kontakt does have it [inferred]. Gap to verify.
- Default keyswitch: "the switch that will be active right after the Instrument has been loaded" [doc: ~4339].

Which parts libraries actually use [lib]:

- Most selection in shipped libraries is scripted: group masks via `allow_group`/`disallow_group` inside `on note`,
  not start criteria. Survey: Areia has about 157 `allow_group($groupcount)` calls in a stateful performance callback
  that also holds RR and legato state; Solo has 132 allow calls and 34 disallow calls; Conflux has no articulation
  set at all and is a MIDI player [docs/re/survey-kontakt-switches.md]. Pacific Ensemble Strings: 41 of 49 NKIs
  have plain-text `allow_group`/`on note` and 29 contain round-robin text [lib, my grep: `strings` over the NKI].
  Performance Samples Vista: 7 of 7 NKIs contain `allow_group` or `on note` text [lib].
- Afflatus Brass: two control keys, "Reset RR" and a breath key that also plays a note after selecting groups
  [docs/re/survey-kontakt-switches.md, corpus 2026-10-22].
- Trombones KS: 6548 attack and 1816 release zones with `sample_start_mod_range` 5120, unrouted
  [docs/re/engine/UNROUTED_START_RANGE.md]. Trills (Afflatus Chapter II Brass / Solo Trumpet): release zones with
  `start` 7680 on GateRelease groups 16-31 [same doc]. Dolce: CC113 "S. Start" slider routes the start
  [same doc]. ANALOG STRINGS, Una Corda, Conflux, Morphology Evolved, Audio Imperia CHORUS and Shreddage E-Upright
  were not scanned for selection beyond counts; the NKI scripts are not plain text for most libraries, so `strings`
  reports nothing and is not evidence of absence.

### 2.2 UVI (light touch)

- The UVIWorkstation executable contains identifiers `RoundRobinMode`, `CrossFade`, `ReleaseTrig`, "Round Robin"
  and "Legato" [binary: `strings` on UVIWorkstationx64.exe, names only]. No laws, default values, or counter scope
  were recovered from it. No value is claimed.
- UVI `.ufs` files (26 over 1 MB, `Augmented Orchestra.ufs` 18.4 GB) are binary. Readable JSON keys are present
  for step sequencer data, but a 60 MB sample showed no selection keys. Not analysed further.
- `/home/derpcat/Documents/UVI/Falcon/UVIScript` is empty on this machine, so there is no UVIScript reference.
- Status: UVI selection law unknown. Do not design UVI behaviour from this study.

### 2.3 HISE

- Round robin is one `MultiGroupState` per sampler: `bumpRoundRobin(rrGroupAmount)` cycles `1..=limit`
  [source: `hise/hi_core/hi_sampler/sampler/ModulatorSampler.h:907-911`]. It is called on every note-on when
  `useRoundRobinCycleLogic` is set and no grouped collector is active [source: `ModulatorSampler.cpp:1497-1501`].
  The counter is bumped before sounds are collected, so it steps on every note-on regardless of match, like Kontakt.
- The number of RR groups is `rrGroupAmount` (`ModulatorSampler.cpp:1938`, `jmax(1, newGroupLimit)`); it is a
  sampler setting, not derived from the rows. Sound lookup is per key and velocity through `RoundRobinMap`
  (`ModulatorSamplerData.cpp:1107-1115`).
- A grouped collector path (`GroupedRoundRobinCollector`, `ModulatorSampler.cpp:2122-2157`) uses pending event IDs
  to restore the RR state for asynchronous notes [source]. This matters for async note handling, not for the
  selection law.
- HISE docs site (`docs.hise.audio`): the fetched pages were landing and API pages with no RR, crossfade or
  release rules [doc, WebFetch, 2026-10-27]. Not used as evidence.

### 2.4 sfizz and the SFZ format

- `seq_length` 1..100, default 1; `seq_position` 1..`seq_length`. The counter is per region and steps only when the
  note is inside that region's key range [source: `Layer.cpp:58-62`; `Defaults.cpp:83`]. The SFZ spec page says
  the player keeps a counter "for each region" and that ARIA does not track it across regions [doc: sfzformat.com
  `/opcodes/seq_length`, WebFetch]. CC-triggered sequences step the same counter on CC change (`Layer.cpp:184-186`).
- Round robin is therefore per-region, key-scoped, and not shared with other regions; this differs from Kontakt and HISE.
- `lorand`/`hirand`: sfizz selects random ranges on `randValue` per note [source: `Layer.cpp`, `registerNoteOn`].
  Not separately measured here; the SFZ opcode definitions were not retrieved (the opcode page fetch returned
  only the header page), so their exact spec text is not cited.
- Crossfades [source: `ModifierHelpers.h:17-60`]: the in-fade is `sqrt(pos)` (power) or `pos` (gain), the out-fade
  `sqrt(1-pos)` or `1-pos`; `pos = distance / (range_length - 1/127)`. Default curve is `power`
  (`Defaults.cpp:208`), so an SFZ author gets equal power unless they set `gain`.
- Key, velocity and CC crossfades are each their own curve (`Region.cpp:508-542`; `Region.h:328-336`). CC crossfades
  (`xfin_loccN`/`xfout_hiccN`) exist in SFZ and in sfizz; Kontakt zones have no CC crossfade.
- Trigger values (attack, release, first, legato, release_key) and `sw_*` keyswitch opcodes: sfizz reads `trigger`
  (`Region.cpp:384-385`); the exact semantics of each value were not extracted in this pass.

### 2.5 Decent Sampler, EastWest/Opus, Koda, Ample, MODO DRUM, EZDrummer

- MODO DRUM `.pak` (`/data/main/Libraries/Modo Drum`): entry names encode `_v<velocity>_rr<n>` (1450 entries, 20
  distinct velocity values) and a stereo bus split `-DRY.wav` (750) and `-WET.wav` (700); choke variants are
  separate folders (`..._Choke/Extra Short/`) [lib]. So drums use round robins across velocity layers, with a
  choke as a separate group, and two mic buses. Lowest-impact check: the pak's own selection table was not decoded.
- Ample `.ascf` (`/data/main/Libraries/Ample`) produced no readable selection strings [lib, `strings`] and was not
  analysed.
- Superior Drummer / EZDrummer: the library lives under `/data/main/Libraries/Superior Drummer/EZDrummer`
  (`.obw`, `.s20`). EZDrummer is not a separate folder in `/data/main/Libraries`. Not analysed.
- Decent Sampler and EastWest/Opus/Koda: no documentation was fetched in this pass. No claim is made about them.

### 2.6 KONTRA now

Data path, translator to engine:

1. `translate.rs:244-311` `start_rules`. Every row becomes an `ir::StartRule` in group order; `next` is read only to
   diagnose OR (`:262-272`). `mode 3` (cycle) gives `position = cycle_class` and `length = max(cycle_class)+1` over
   all rows (`:253-256`, `:292-297`). `mode 4` (random) gives `position` = row ordinal and `length` = **number of
   groups containing a random row** (`:257-258`, `:298-305`). Key rows keep only `key_min` as `key` and the range
   as `lo..=hi` (`:274-287`).
2. `ir/src/lib.rs:234-247` documents `StartRule` as "every one of which must hold", AND-only.
3. `prepare.rs:37-42` builds `plan.keyswitches` as the min-max span of all keyswitch ranges. `prepare.rs:226-243`
   maps each rule onto `Zone`: keyswitch and round robin are single slots and the last one wins; controllers
   accumulate. Round robin is always `RrCounter::Global` (`:236`).
4. `plan.rs:37-45` `RoundRobin { counter, position, length, random }`; `Zone` holds one `Option<RoundRobin>`
   (`plan.rs:127`). Two RR rows in one group cannot be represented.
5. `behaviour.rs:79-159`: draws once per note-on (`draw`, `:79-86`), xorshift64* (`:89-94`), sequential count
   snapshot at note-on (`:133-139`), no-repeat for random (`:126-130`), per-counter `used`/`pending` (`:14`, `:124-143`).
   `advance()` steps once per counter that any zone read (`:147-159`).
6. `lib.rs:385-436` `start_zones`: predicate order is accept(trigger) → mask → key → velocity → keyswitch → CC ranges →
   bus. Only then does `round_robin_plays` run (`:393-411`), and it marks the counter `used` (behaviour.rs:124-125).
   A note that matches no zone does not step any counter.
7. `lib.rs:416-418` release factor `10^(-release_decay_db * held/20)`. `held` = time from note-on to release. The
   field is never set by the translator (`prepare.rs:250`; no producer in `crates/format`), so the factor is 1.
8. `lib.rs:422-425` start offset from script `play_note(offset)` (`script.rs:288`), in microseconds. The IR
   `start_mod` only affects the preload window (`prepare.rs:164-188`).
9. Crossfade `plan.rs:55-81`: `sin` in, `cos` out, equal power. `prepare.rs:211-219` builds the velocity fade with
   `(low-1, low+F)` and `(H-F, H+1)` ramps. Those bounds reproduce Kontakt's `(v-L+1)/(F+1)` once the curve is
   linear (checked by reading the bounds). Key crossfades: no IR field, dropped at `translate.rs:360-362`.
10. Trigger: attack/release from `group.release_trigger` (`translate.rs:178-181`). `First`/`Legato` are engine-only
    (`script.rs:225-229`, from `held` count `:215-219`); the IR never produces them.
11. Keyswitch: note-on sets `behaviour.keyswitch` for any key in the span (`lib.rs:361-363`); a keyswitch note
    sounds nothing (`lib.rs:389-391`); `plan.default_keyswitch` is the minimum switch of any zone
    (`prepare.rs:75`), not Kontakt's default switch.
12. Tests that pin current behaviour (so they will need to change): `crates/engine/engine/src/plan.rs:246-263`
    (equal power), `crates/engine/engine/tests/behaviour.rs:270-285` (equal power at midpoint),
    `behaviour.rs:192-200` (keyswitch span 24..=35, all keys silent).

## 3. Gaps, ranked by audible impact

Impact is my judgement from how often the behaviour appears and how big the level or note error is.
Measurement column = the check that would prove it. None were run (no build allowed for this task).

| # | Gap | Where | Kontakt / reference | Impact | Measurement that settles it |
|---|---|---|---|---|---|
| 1 | Velocity crossfade law is equal power; Kontakt is linear | `plan.rs:77-80`; `ir` comment `lib.rs:255-257` | KONTAKT_REFERENCE s.11 (linear, first step 1/(F+1)) | High: every velocity-layered library with fades; mid-fade gain differs by about 3 dB | Render a one-zone noise instrument with fade F=60 at v=40, 48, 56, 64, 80 with Kontakt oracle and KONTRA; compare RMS dB. Pass: within 0.5 dB. |
| 2 | Key crossfades dropped | `translate.rs:360-362` (diagnosed Audible only) | s.15: same linear ramp, measured within about 1 dB | High on key-switched multi-samples (common) | As 1, with `ZONE_PAR_FADE_HIGH_KEY` 36 at H=72; expected -22 dB at key 72 vs key 60. |
| 3 | AND NOT silently becomes AND | `translate.rs:262-265` (no branch for `next == 1`) | Stored `next` 1 = AND NOT (binary, KSP enum) | High where used (pedal split libraries, `manual ~6580`) | Fixture: two groups, A and NOT B on a controller. Kontakt: group plays only when B off. KONTRA: plays on A alone. |
| 4 | OR joins are ANDed after a diagnostic | `translate.rs:266-272` (diagnostic), `ir` AND-only `lib.rs:234-247` | `OR_NEXT` (binary `0x144f18228`) | High (silent groups) | Two key rows joined by OR: play one key of each, expect both groups. KONTRA: neither. |
| 5 | Random count uses groups, not rows; two random rows in one group lose one | `translate.rs:257-258`; `prepare.rs:230-241` (last RR wins) | N = rows, all slots (binary `0x140876bd0`) | High in drum layers with random rows | Fixture: one group, two random rows (ordinals 0,1). Expect N = 2. KONTRA N = 1, so ordinal 1 never sounds. |
| 6 | Keyswitch silences every key in the span, not only switch keys | `prepare.rs:37-42`, `lib.rs:353-358`, `lib.rs:389-391` | Manual ~980-981: keyswitches are the only keys that do not sound | High for instruments with switches inside the playing range | Keyswitch rows at keys 24 and 36; play key 30 on an ordinary zone. Expect sound; KONTRA silent. |
| 7 | Key-range keyswitch collapses to its lowest key | `translate.rs:274-287`, `prepare.rs:228` | Start on Key = "trigger note within the defined range" (manual ~6611) | Medium-high (Diagnosed as Audible; keys 25..27 do nothing) | Range 24..27 on one group; press 26; expect the group active. |
| 8 | Release-trigger counter attenuation not translated; engine law is dB/s, not linear | `prepare.rs:250` (no producer), `lib.rs:416-418` | Manual ~8526-8531; BASIC_BEHAVIORS row 6 (linear `(T-X)/T`) | Medium (release tails too loud for note-length dependent libraries) | Release-trigger group with counter route, hold for 0.2 s, 1 s, 3 s. Measure release RMS vs Kontakt; fit law. |
| 9 | Round robin steps only on matched notes | `lib.rs:393-411`, `behaviour.rs:124-125` | Kontakt steps on every note-on, before predicates (binary, `0x144f17ea0`); HISE same (`ModulatorSampler.cpp:1497-1501`) | Medium: phase drift when RR groups share keys with non-RR groups or velocity splits | Oracle test: RR zone at keys 36-48, plain zone at 60-72. Play 60 then 36. Kontakt position after one note on key 60 tells whether the counter stepped. |
| 10 | Random row no-repeat and xorshift instead of Kontakt's uniform MT draw | `behaviour.rs:89-94`, `:126-130` | No no-repeat, MT19937, `random(0,N-1)` (binary) | Medium (repeat rate audibly different on machine-gun tests) | Over 10000 note-ons on a 4-layer random group: histogram and max run length vs Kontakt. |
| 11 | Random pick reused on release; Kontakt redraws on release callbacks | `lib.rs:383` (re-read attack draw), `behaviour.rs` `RrDraw` | Binary: release-generated callbacks make a new random selection | Low-medium | Random-layered release group; count distinct release layers per note over 200 notes. |
| 12 | Sample start random offset (Kontakt S.Mod) not played | `prepare.rs:164-188` (window only), `lib.rs:422-425` (script offset only) | Trombones KS 5120 frames; Kontakt law open (UNROUTED doc, test inconclusive) | Medium on brass and attacks with start range | Needs a clean oracle capture (single RR layer, no release overlap, RR off), per UNROUTED doc open items. |
| 13 | Monophonic release (cut earlier release samples) absent | no `mono` in `crates/engine` | Manual ~8532-8535; binary 10 ms fade | Medium on pianos and drums with release samples | Repeated note on release-trigger group: count sounding release voices. Expect 1 when monophonic. |
| 14 | Legato playback-position carry-over absent (group legato) | no carry in `crates/engine`; only note-level `Legato` trigger `script.rs:225-229` | Manual ~8746-8748; melody engine ~3775-3781 | Medium for legato sample sets | Two legato notes, same zone, measure continuity of the second note's start vs Kontakt. |
| 15 | Round-robin counters always global in the plan | `prepare.rs:236`; `behaviour.rs:96-106` (Key/Group unreachable from IR) | Kontakt global per instrument; HISE global per sampler; SFZ per region (sfizz `Layer.cpp:58-62`) | Medium for SFZ, zero for Kontakt | Port check once SFZ translator exists: two regions sharing key ranges and seq_length; counts must follow the region's own counter. |
| 16 | `First`/`Legato` triggers exist only in the script path; no IR producer | `script.rs:225-229`; `translate.rs` never sets them | Kontakt: legato via the group button and interval; SFZ: `trigger=first/legato` | Medium on legato articulations | Fixture: legato-trigger zone plus attack zone; play overlapping and non-overlapping notes. |
| 17 | Default keyswitch = minimum switch | `prepare.rs:75` | Manual ~4339: "the switch active right after load" (user-set) | Low-medium | Load an instrument with switches 24, 25; check the first note's articulation against Kontakt. |
| 18 | Mixed AND/OR precedence unproved (not a gap in KONTRA yet, a risk for the translator) | — | binary: unproved; manual gives truth tables only | Unknown until traced | Oracle matrix of `A or B and C` against Kontakt, all eight combinations of A, B, C. |

## 4. Gold standard: one selection model

Principle: the engine evaluates one expression per zone per note; every format translates its own rules into
that expression and its fades, counters and offsets. The engine has no format knowledge (as now, `plan.rs:1-3`).
The model below replaces `StartRule`, `RoundRobin` and the single-slot `Zone` fields.

```rust
/// What a zone needs at note-on. A zone sounds when `expr` is true.
pub struct Selection {
    pub expr: Expr,                 // explicit tree; no implicit precedence
    pub trigger: Trigger,           // Attack | Release(ReleaseSpec) | First | Legato | Transition(RangeInclusive<u8>)
    pub fades: Vec<Fade>,           // velocity, key, cc; each its own law
    pub start: StartOffset,         // Fixed(frames) | Random { range_frames, law: Option<OffsetLaw> }
}

pub enum Expr {
    Atom(Atom),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    True,                           // "Always"
}

pub enum Atom {
    Key(RangeInclusive<u8>),                    // per note key
    Velocity(RangeInclusive<u8>),               // per note velocity
    Cc { cc: u8, range: RangeInclusive<u8> },   // live, inclusive
    Switch { range: RangeInclusive<u8>, active: u8 }, // articulation latch: last switch key in `range` is `active`
    Cycle { counter: CounterId, position: u16, length: u16 }, // step `position` of `length`
    Pick { pool: PoolId, ordinal: u16 },        // random: draw over pool size, match ordinal
}

/// Cycle state. Scope and step rule come from the format, not from the engine.
pub struct Counter {
    pub scope: Scope,                // Instrument | Group | Key | Region
    pub steps_on: StepOn,            // EveryNoteOn | InRangeNoteOn
    pub strict: bool,                // true: releases reuse the attack step
}

pub struct Fade {
    pub control: Control,            // Velocity | Key | Cc(u8)
    pub law: Law,                    // Linear | EqualPower | DbLinear
    pub low: Ramp, pub high: Ramp,   // fade-in and fade-out bounds in control units
}

pub enum Trigger { Attack, Release(ReleaseSpec), First, Legato, Transition(RangeInclusive<u8>) }
pub struct ReleaseSpec { pub decay: Decay, pub mono: bool }
pub enum Decay { None, LinearHeldMs { t_ms: f32 }, DbPerSecond(f32) }
```

Semantics the engine must implement (each traced to a reference):

1. A zone sounds if `expr` is true, `fades` give the gain, and `start` gives the offset. A note with no true zone
   steps no counter unless the counter's `steps_on` says otherwise (Kontakt and HISE: every note-on; SFZ: key in
   range).
2. `Switch` is a latch of the last switch key in the range. Keys inside a switch range that are not switch keys still
   play normal zones. A note sounds nothing only if it is itself a switch key (Kontakt manual ~980-981).
3. `Cycle` and `Pick` are evaluated against the counter value and draw read at note-on; release triggers reuse
   them when `strict` is true (matches binary `0x144f17920` and `behaviour.rs:136-137`).
4. `Pick` uses a per-instrument draw over the pool size, with no no-repeat, so Kontakt's N is `pool.len()`. A
   format that wants no-repeat (any future format) adds an `OffsetLaw`-style option on the pool, not in the engine.
5. Fades: Kontakt velocity and key fade = `Linear` with bounds `(L-1, L+F)` and `(H-F, H+1)` (reproduces
   `(v-L+1)/(F+1)`, checked by reading `prepare.rs:211-219`). SFZ default = `EqualPower` (`sqrt`), `xfin_*` with
   `gain` = `Linear`. The law belongs to the format, not the engine.
6. Release: `Decay::LinearHeldMs` = `max(T - X, 0)/T` on note-off (binary `0x14084f380`), `DbPerSecond` for any
   format that states dB/s.

Format translation:

| Gold concept | Kontakt 8 | SFZ (sfizz) | HISE | UVI |
|---|---|---|---|---|
| `Key` / `Velocity` | zone key/vel ranges | `lokey`/`hivel` | sample map ranges | zone ranges (law not recovered) |
| `Switch` | Start on Key rows; switch keys do not sound | `sw_lokey`/`sw_last`/`sw_previous` (sfizz `Layer.cpp` keySwitched) | script `setKeyswitch`-style | unknown |
| `Cycle` (Instrument) | mode 3 rows, N = max position + 1, step on every note-on | — | `RoundRobinMap` + `bumpRoundRobin`, step on every note-on | `RoundRobinMode` (name only) |
| `Cycle` (Region) | — | `seq_length`/`seq_position`, step on in-range note-on | — | — |
| `Pick` | mode 4 rows, N = row count, MT19937 draw | `lorand`/`hirand` (per-note random) | script `random()` | unknown |
| `Fade` | zone velocity/key fades, linear | `xfin/xfout`, default `power` | sample map crossfade groups | `CrossFade` (name only) |
| `Trigger::Release` | group release trigger, counter | `trigger=release`, `rt_decay` | release sample groups | `ReleaseTrig` (name only) |

What would change in KONTRA (for the next implementer, not done here):

- `ir::StartRule` becomes `Expr` over `Atom`; `translate.rs:244-311` emits trees; the OR, AND NOT and mixed-precedence
  cases become explicit nodes once the precedence is traced (gap 18).
- `Zone.round_robin: Option<RoundRobin>` (`plan.rs:127`) becomes `Vec<Atom>` plus a counter table; the
  `RrCounter` enum (`plan.rs:25-34`) is replaced by `Scope`.
- `Crossfade::gain` (`plan.rs:67-81`) takes a `Law`. Kontakt translator sets `Linear`. Key crossfades get an IR
  field (gap 2).
- The release gain path (`lib.rs:416-418`) takes `Decay` instead of a bare dB/s value; the translator fills it from
  the group's counter route (gap 8).
- The random sample-start offset gets a `StartOffset::Random` and an oracle-validated law (gap 12); until then it
  stays unplayed and diagnosed, not silently dropped.

## 5. Measurements to run (none run here)

Each claim below is either [binary]/[doc]-tagged or [inferred]. The run column is the test that would make it
[measured].

| Claim | Status | Run |
|---|---|---|
| Kontakt linear velocity fade law | [measured] in KONTAKT_REFERENCE s.11 | Re-run as gap 1, use it as the regression for `Law::Linear`. |
| Kontakt random N = rows, MT stream | [binary], `docs/re` | Oracle: 4-row random group, 1000 notes, compare to KONTRA after the N fix (gap 5). |
| Kontakt RR counter steps on every note | [binary] | Gap 9 oracle fixture. |
| Kontakt Start-on-Key latch semantics | [binary] + [doc] | Keys in a gap between two switch ranges (gap 6, 7). |
| SFZ per-region seq counter | [source] sfizz | sfizz run on two overlapping regions with seq_length 2 (no Kontakt needed). |
| HISE per-sampler RR counter | [source] | HISE headless render; only if needed, the source is enough to settle scope. |
| Release counter law `(T-X)/T` | [binary] | Gap 8 run at T = 0.2, 1, 3 s. |
| Mono release cut | [doc] + [binary] | Gap 13 oracle run. |

## 6. Open items

- Kontakt AND/OR precedence and associativity: needs a parser-table trace or an oracle matrix (gap 18).
- Kontakt random seed: none fixed; KONTRA's `reset_native_cycles` seed is a choice, not Kontakt's
  (`docs/re/engine/BASIC_BEHAVIORS.md` row 3).
- Kontakt sample-start random law: unresolved (UNROUTED doc, test inconclusive).
- Kontakt legato intervals and previous-key storage: not verified in this study.
- Whether a Kontakt keyswitch note increments the RR counter: not established (binary says release-generated
  callbacks reuse; keyswitch callbacks are not covered).
- UVI selection law: no recovered source. Needs UVIScript docs or a UVI oracle.
- NKI scripts are not plain text for most libraries; a selection census needs the `ni_file` reader, not `strings`.
- Ample, EZDrummer/Superior Drummer and Decent Sampler not analysed here.
- Scripts that call `allow_group` drive selection outside the declarative model. The gold model above covers only
  start criteria; the script surface (`crates/engine/engine/src/script.rs`) owns it.
