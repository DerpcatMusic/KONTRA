# 13 Fresh core: rebuild the fundamentals, don't move them

Status: accepted by the maintainer, 2026-10-27. This doc changes 11-MIGRATION.md
phases 4–8 and adds a behaviour layer that 01–07 don't have.

## 1. Where the plan falls short

| Gap | Today's plan | Why it isn't enough |
|---|---|---|
| **Moves, not rebuilds** | Phases 4–8 `git mv` `sampler-core` (135k lines) into new crates with re-exports. | The fragility moves with the code: cold holds, cohorts, fuel and 109 `Runtime` fields. Phases 1–3 patched symptoms inside that code. |
| **No behaviour model** | Round robin, legato, release triggers, crossfades and polyphony limits are in no doc. Voice groups appear only in passing. | Every serious sampler makes these engine primitives: SFZ opcodes, UVI, EastWest Opus and HISE. KONTRA leaves them to KSP and Lua scripts, so a script bug or stall becomes a wrong note. |
| **Weak voice ownership** | A ledger maps notes to voices. | A note's mic positions and layers must start, move and stop as one unit, by construction. |
| **No modulation model** | Kontakt modulators are a DSP family. | UVI, Opus and HISE have one modulation matrix: any source to any parameter, per voice or per note. Kontakt's modulators are one instance of it. |

## 2. The fresh core

New crates are written to these docs, not moved. The old `sampler-core` keeps
playing until the new core passes the probe contracts (10-TESTING.md) on the
reference libraries. Then the plugin switches and `sampler-core` is deleted in
one PR. Code from the old core is copied only where it is measured to be right,
for example the DFD law (`dfd_window.rs`) and RE'd DSP laws.

The audio path, layer by layer:

```text
host/UMP event ─► Engine queue (sample time) ─► Note (ledger)
   ─► Behaviour: articulation state, key/vel/CC rules, round robin, legato,
      release trigger, crossfade weights, voice-group limits
   ─► VoiceSet: every voice one note instant starts (mics, layers), one start
      sample, one release, one owner
   ─► Voice: cursor + interpolation + envelope (kontra-voice)
   ─► Bus: per-mic and per-group mixers, insert chains (kontra-dsp)
```

### 2.1 Behaviour is data, scripts are optional

`kontra-ir` gains a behaviour block per articulation. This is SFZ's proven set,
named for what each does:

- **Selection:** key range, velocity range, CC range, keyswitch, articulation state.
- **Round robin:** sequential or random, per key or per group, a reset rule.
- **Crossfades:** velocity, CC or note-expression layer crossfades (dynamics), equal power.
- **Triggers:** attack, release (with release-time decay), first, legato.
- **Legato:** mono, with a transition-sample table and a glide time.
- **Voice groups:** polyphony per group, choke or "off by" groups (hi-hat), mono and steal law.
- **Mic positions:** named outputs of one articulation, mixed live (Opus-style), purgeable.

Format translators map onto it. SFZ maps nearly 1:1, UVI keygroup and layer
rules map directly, and Kontakt maps group start options, release triggers and
voice groups. KSP and Lua run on top through `HostApi`: `play_note`,
`ignore_event` and `set_engine_par` call the same primitives. A library whose
script only reimplements round robin or release triggers can run with the
script's note handling bypassed and still play correctly.

### 2.2 Voice ownership

- **A note owns its `VoiceSet`; a set owns its voices.** Release, choke,
  expression and note end go note → set → voices. No voice is ever addressed
  any other way.
- **One start sample per set.** All of a set's voices start at the same
  sample, so the double attack can't happen.
- **Shared timeline.** Mic positions of one zone are separate voices with the
  same cursor anchor. The engine may merge them into one multi-region voice as
  an optimisation, without changing the output.

### 2.3 MIDI and expression

- UMP-native input: MIDI 1, MIDI 2, MPE and CLAP note expression become one
  per-note expression state on the `Note`.
- CC, pitch bend and aftertouch are queue events at their sample, smoothed
  per destination. There is no per-block CC sampling.

### 2.4 Modulation

`kontra-dsp-modulators` provides one matrix.

- **Sources:** velocity, key, CC, note expression, envelopes, LFOs, random, round-robin index and legato state.
- **Destinations:** any `#[derive(Params)]` parameter, per voice or per bus.
- **Kontakt:** its modulator list, and UVI's, are rows in this matrix.

## 3. No bridges

A new crate never depends on a `sampler-*` crate, not even temporarily.
Correct old code (RE'd laws, decoders, the KSP grammar) is rewritten cleanly
into the new crate, with its tests. The foundation is built and proven bottom
up, and only then wired into the plugin. A probe of the old IR on the new
engine (2026-10-27, not merged) showed why scripts belong in the foundation:
Areia Spiccato Fast has 3 dynamic layers × 8 round robins per key, and its KSP
picks one of the 24 groups. Without the script, all 24 sound.

## 4. Build order (each step: tests pass, then the next)

1. ✅ `kontra-stream`: pages, pins, cache, I/O; never waits (#833).
2. ✅ `kontra-voice`: cursor, loops, reverse, Hermite, AHDSR; any-block-size cursor.
3. ✅ `kontra-engine`: queue, ledger, owned voices, steal law with fades, render
   loop; behaviour (round robin, crossfades, triggers, keyswitches, voice
   groups, choke, mic buses) as plan data.
4. `kontra-vm`: bytecode, interpreter, coroutine `wait`, fixed memory, `HostApi`.
5. `kontra-ksp`: lexer, parser, resolver, lowering to `kontra-vm`, rewritten
   from the KSP manual and the old front end's tests.
6. Engine script host: callbacks at their sample; `play_note`, `ignore_event`,
   group allow lists, event parameters, `wait`, engine parameters; UI writes to
   a latest-value control store.
7. `kontra-ir`: the format-neutral instrument (zones, groups, behaviour,
   scripts, buses, modulators); engine prepare compiles it into a `Plan`.
8. Fresh format crates onto `kontra-ir`: audio decoders (WAV, AIFF, FLAC, NCW),
   then the Kontakt container (NKI/NKM/NKR, monoliths), then UVI and SFZ.
9. Modulation matrix and the DSP families.
10. Wire: host load pipeline, plugin shell, UI on the protocol. Switch on probe
    parity (Areia, Vista, ANALOG, a UVI library); delete every `sampler-*` crate.
