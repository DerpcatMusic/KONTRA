# KONTRA: the sampler of the future, plan

Goal: the best sampler: plays Kontakt and UVI libraries faithfully, with
Kontakt's timing laws, UVI's scripting model, HISE's compiled DSP, KODA's
authoring and Opus/SINE's footprint, built so a playback bug is impossible
by construction, not counted after the fact.

Inputs: [PLAYBACK_AUDIT.md](PLAYBACK_AUDIT.md) (what is broken and why), [ONE_ENGINE.md](ONE_ENGINE.md) (one runtime, thin translators, memory, CPU),
[SAMPLER_LANDSCAPE.md](SAMPLER_LANDSCAPE.md) (what each competitor does best).

## 1. The invariants

Every module is judged against these. Each has a probe in CI (section 5).

1. **On-time start.** A voice starts at its event's sample, from RAM, every
   time. Storage can cause a dropout later; it can never move an onset.
   All voices of one musical note (every mic, layer and `play_note` in the same
   callback instant) start on the same sample.
2. **One clock.** Sample time is the only time. Host blocks, render fragments
   and UI ticks never quantize an event, wait, parameter change or engine edit.
3. **Scripts run to completion.** A callback runs at its event time until it
   ends or waits. The only guard is Kontakt's `while` iteration limit. No
   per-block instruction budget, no abort that releases a note.
4. **UI never touches audio timing.** Script UI writes go to a latest-value
   store the GUI reads. Engine edits (`set_engine_par`, group/zone changes) are
   applied by the audio thread at their sample. Nothing waits for the UI.
5. **One owner per note.** One ledger maps host note → script event → voices.
   Releases, chokes, expressions and steals look it up; nothing else tracks notes.
6. **Offline equals real time.** Same events in, same samples out, at any block
   size. Streaming may only change *quality* (a counted dropout), never timing.
7. **Honest memory.** Preloads in the source's native format (16/24-bit or
   compressed), sized by a stated law, reported in the UI with their RAM cost.
8. **One implementation per concept.** One VM, one voice renderer, one
   streaming path, one DSP class per effect type (ENGINE.md "KISS and YAGNI").

## 2. What we take from whom

| From | Take | Why |
|---|---|---|
| Kontakt | DFD law: fixed per-zone preload + the zone's start-modulation range; script offsets capped to it. Run-to-completion KSP. `ignore_event` before the first `wait` decides the original note. | It is what libraries are authored against; it makes invariant 1 cheap. |
| HISE | Preload + two swapped streaming buffers per voice; whole-load for start-modulated ranges; "author dynamic, ship compiled" DSP. | Proven open design; voice cost independent of disk. |
| UVI Falcon | Callbacks as coroutines with sample-accurate `wait`; scripts as stackable modules; the module tree. | The cleanest event-scripting model in the industry. |
| KODA | Articulation → Variation → Group hierarchy; `NoteData` template (build a note, then start it); parameter events coalesced; `persist` + hot reload; Figma UI import; LITE packages; lane editor; AI-friendly docs. | The best authoring story, and the one creators talk about. |
| Opus / SINE | Per-instrument and per-mic downloads; a preload slider that shows RAM; Mic Merge bake. | Footprint is the main complaint about big libraries. |
| Synchron | Dimension tree for articulations; per-mic delay and phase in the mixer. | Orchestral ergonomics; layer alignment is a control, not a bug. |
| sfizz / SFZ | Open, diffable text format; reclaim idle sample memory. | Our native format should be open and reviewable. |
| Modeling / neural | Hybrid zones: sampled attack + modeled/neural sustain and transitions. | Where the next decade goes; the IR must allow it now. |

What we do that nobody does: **play other vendors' formats faithfully** and
**prove timing in CI**.

## 3. Architecture

```
 host (CLAP/VST3/AU)          formats (load, off audio)
   │ events w/ sample offset     kontakt │ uvi │ sfz │ kontra-native
   ▼                                     ▼
 ┌──────────── plugin shell ───────┐   instrument IR (one model)
 │ params, state, editor bridge    │   Articulation→Variation→Group→Zone,
 └──────────────┬──────────────────┘   mics, buses, modulation, scripts
                ▼                             │ prepare (off audio)
 ┌──────────── engine (audio thread) ─────────▼────────────────────┐
 │ clock ── event queue (sample-stamped, one ordered stream)      │
 │   │                                                             │
 │   ├─ script host ── VM (run to completion, coroutine waits)    │
 │   │        │ engine edits (sample-stamped) │ UI writes → store │
 │   ├─ note ledger (host note ↔ script event ↔ voices)           │
 │   ├─ voice allocator / stealing                                 │
 │   ├─ voices: source (preload → stream buffers) → mod → DSP     │
 │   └─ buses / mixer / FX (one DSP library)                       │
 └──────────────┬──────────────────────────────────────────────────┘
                │ page requests (lock-free)     ▲ filled buffers
                ▼                               │
          stream service (I/O threads: read, decrypt, decode into native format)
```

Scripting, three tiers on **one VM**:

- **Compat front ends:** KSP and UVIScript (Lua) compile to the KONTRA VM.
  The existing `sampler-core` behavior bytecode is the right base:
  allocation-free, bounded. Fix its *policy* (invariants 3–4) and run
  `on init` on it too, then delete `eval.rs`.
- **Native script (new libraries):** a typed, modern language for the
  KONTRA format: `NoteData`-style note templates, structs, coroutine waits,
  `persist`. Candidate: Luau syntax and types compiled to the same VM (not
  the Luau runtime: no GC on the audio thread). Decide after phase 3 with a
  measured spike.
- **DSP tier:** Rust modules in one DSP library; user DSP later via a
  compiled graph (Cmajor/Faust-class, JIT for authoring, AOT for shipping).
  YAGNI until a library needs it.

## 4. Folder plan

Target layout (each crate one concept; names say what it is):

```
crates/
  clock/           sample time, transport, tempo maps, event timestamps
  events/          host + MIDI 1/2 + CLAP note-expression ingress → one event type   (from sampler-midi, src/sound/event.rs)
  ir/              the instrument model; no format knowledge                            (sampler-ir)
  engine/          audio thread: queue, ledger, allocator, voices, render              (sampler-core: lib.rs/render*/source*/ownership/steal/onset→deleted)
  stream/          preload store + I/O service + decode-to-native; the DFD law         (sampler-core/stream.rs, residency*, head_*; sampler-kontakt/stream.rs I/O half)
  dsp/             every filter/EQ/dynamics/reverb/delay class, once                    (sampler-core/dsp, sampler-simd, sampler-kontakt/effects laws as parameters)
  modulation/      envelopes, LFOs, modulators, routing                                  (sampler-core/blocks, voice_mod*, modulation*)
  vm/              script VM: bytecode, scheduler (coroutines, waits), host API traits  (sampler-core/behavior*, ops.rs)
  script-ksp/      KSP lexer/parser/sema → vm bytecode, init included                   (sampler-ksp minus eval.rs, luau/)
  script-uvi/      UVIScript (Lua) → vm bytecode                                         (sampler-uvi script parts)
  format-kontakt/  NKI/NKM/NKS/NCW/monolith readers → ir                                (sampler-kontakt minus stream I/O and effects DSP)
  format-uvi/      UFS/UVI readers → ir                                                  (sampler-uvi readers)
  format-sfz/      SFZ ↔ ir (open, diffable)                                             (new, small)
  format-kontra/   native package: ir + scripts + native-format audio, LITE/per-mic      (new)
  ui-model/        widget tree + latest-value control store (script UI writes land here) (sampler-ui-ir, script_params, widget)
  pool/            arenas/handles shared by engine and vm                                 (sampler-pool)
  probe/           playback contract harness (section 5), CLI + CI test               (sampler-perf, sampler-parity, dspbench, the audit probe)
src/               plugin shell only: params, state, editor, host glue                  (src/plugin.rs, src/sound/v2.rs shrink to wiring)
src/ui/            editor views                                                         (unchanged)
docs/              ENGINE.md, FORMAT.md, SCRIPTING.md, CONTRACTS.md; dated audits under docs/audit-*/
```

Rules: crates depend downward only (`plugin → engine → {vm, stream, dsp,
modulation} → {ir, clock, pool}`; formats → ir). No crate below `engine`
knows Kontakt or UVI. Vendor laws (Kontakt's curves, UVI's ranges) are
*parameters* loaded by `format-*` into the one `dsp`/`modulation` class.

Moves are renames plus deletions, done crate by crate behind green probes.
Not a rewrite from zero: the readers, DSP laws, KSP front end and UI are assets.

## 5. Contracts we measure (the probe)

`crates/probe` grows the audit probe into a gate. Each contract reports one
number per instrument, run on a pinned library set (Areia, Vista, Conflux,
Analog Strings, Una Corda, two UVI programs), real-time mode, OS cache dropped
where the machine allows, block sizes 32/128/1024:

| Contract | Pass |
|---|---|
| Onset error: first audible sample vs event sample, every voice | 0 samples |
| Layer alignment: spread of onsets across one note's voices | 0 samples |
| Callback aborts / note releases from script faults | 0 |
| Dense vs live: chords audible / notes audible | 100% / 100% |
| Offline vs real time: rendered difference | bit-identical, or −120 dB with streaming dropouts counted separately |
| Dropouts at 256 voices streaming from NVMe | 0 |
| Preload RAM per sample start (16-bit stereo, 60 KB law) | ≤ the stated law |
| Script UI writes per note: audio cost | O(1), no queueing |

A failing contract blocks a merge. Counters (`cold_starts`, `late_attacks`,
`max_onset_wait_frames` …) are deleted with the mechanisms that needed them.

## 6. Phases

| Phase | Deliverable | Deletes | Gate |
|---|---|---|---|
| 0 | Probe as a CI-runnable harness with the contract table | — | Reports today's numbers (the audit's table) |
| 1 | **On-time start.** Per-zone preload in native format: `max(60 KB law, disk-latency law)` + the zone's start-mod range; script offsets capped to it as in Kontakt; per-voice double stream buffers | cold holds, start runway, onset cohorts, head binary search, f32 heads | Onset error 0, layer alignment 0 on Areia/Vista |
| 2 | **UI/audio split.** Latest-value UI store; every engine edit applied in audio at its sample | effect outbox for UI, `Coverage::Effect` engine edits, outbox abort | Areia dense = live; zero stalls |
| 3 | **Run to completion.** Drop the block budget; keep `LOOP_LIMIT`; coroutine waits | fuel allowance, `BehaviorOverload`, defer queue | Zero aborts on the library set |
| 4 | **One note ledger** | `held`, `consumed_hosts` duplicates, aligner host table special cases | `V2Core::event` < 100 lines |
| 5 | **One VM.** `on init` on the VM; KSP and UVIScript both lower to it | `eval.rs`, `luau/` KSP path | KSP conformance suite green on both entry points |
| 6 | **Folder plan** (section 4), crate by crate | dead render variants, "v1" shims | Probe green after each move |
| 7 | **Native format + authoring:** `format-kontra`, SFZ, Articulation→Variation→Group editor, Figma import, LITE/per-mic packages, mic merge | — | A new library authored without code |
| 8 | **Future:** native typed script; compiled user DSP; hybrid modeled/neural zones; MIDI 2.0/CLAP per-note everything | — | One shipped hybrid instrument |

Phases 1–3 fix what users hear today and are small: mostly deletions plus one
preload law. Phases 4–6 make the code base something one person can hold.
Phases 7–8 are where KONTRA passes KODA.

## 7. Open questions (decide with measurements, not taste)

- Preload law: the Kontakt 60 KB constant, or a disk-latency law (p99 cold read
  × max pitch step), whichever is larger? Measure cold-cache p99 on NVMe and HDD.
- Native script syntax: Luau-typed vs a KSP-like DSL (KODA's choice). Spike both
  on one Areia-size script and compare VM speed and author ergonomics.
- Compressed preloads: decode NCW/FLAC heads at load (RAM) vs on voice start
  (CPU). Measure both on Analog Strings.
- Neural/model zones: an IR hook now (a zone `source` variant), engine later.
