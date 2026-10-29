# One engine, thin translators

The goal: one runtime, one DSP library and one sample store that every
sampler format runs on. Kontakt, UVI, SFZ, our own format and whatever comes
next each add only a **translator**: data in, data out, no behaviour of
its own. This doc audits how close we are and specifies the target for
runtime, memory, CPU and future-proofing.

Companion docs: [PLAYBACK_AUDIT.md](PLAYBACK_AUDIT.md) (what breaks),
[SAMPLER_LANDSCAPE.md](SAMPLER_LANDSCAPE.md) (who does what best),
[FUTURE_SAMPLER_PLAN.md](FUTURE_SAMPLER_PLAN.md) (phases and folders).

## 1. Where we are: half shared

| Layer | Shared today? | Evidence |
|---|---|---|
| Instrument model | **Yes**, one IR (`sampler-ir`) | Kontakt and UVI both lower to `sampler_ir::Instrument` |
| Voice runtime | **Yes**, one `sampler-core::Runtime` | Both formats play through it |
| DSP location | **Yes**, all in `sampler-core/src/dsp` (48.7k lines) | Format crates only map fields to IR configs |
| DSP *design* | **No**, ported per vendor product | `filter/kontaktfx.rs`, `timemod/uvi_legacy.rs`, `replika/`, `color/{skreamer,tubescreamer,freak}.rs`, three ladder files; the IR's `Processor` has ~26 kinds, about 9 of them filters (`Filter`, `LadderLP4`, `Daft`, `Ar`, `Formant`, `FilterBank`, `Phaser`, `LivePhaser`, `LiveFormant`); `fx::timemod::Native` has `UviDualDelay`, `UviLegacyDelay` |
| Script runtime | **No**, two runtimes with different timing | KSP: bytecode on the audio thread. UVI: Luau (`mlua`) **on its own thread**; "commands that come back at the next block, so a script reacts within a block or two"; waits polled every 1 ms (`sampler-uvi/src/scripted/thread.rs`). Offline uses an inline host instead, so **UVI offline ≠ real time** |
| Note entry | **No**, two paths | `src/sound/v2.rs` `deliver`: scripted parts take a separate branch (`part.script`) from KSP/plain parts |
| Streaming | **Misplaced** | The stream service lives in `sampler-kontakt/src/stream.rs` (5.1k lines); `sampler-uvi` imports `StreamPolicy`, `Streamed`, `AssetSource` from the Kontakt crate |
| Translators | **Not thin** | `sampler-kontakt` 40.3k lines, `sampler-uvi` 36.5k. Much of it is necessary binary reading, but it also holds streaming, caching, script hosting and UI |

Verdict: the *bottom* (IR, voice runtime) is shared. The *middle* (scripts,
note entry, streaming policy) forks per format, and DSP is organized by which
vendor effect it clones instead of by what it does. The translators are not a
tiny layer; they are two engines' worth of policy.

## 2. The translator contract

A format crate may contain exactly four things:

1. **Readers.** Bytes → typed records (NKI/NKM/NCW/monolith, UFS/UVI XML,
   SFZ text). Large where formats are hostile (encryption, compression);
   that size is fine because it is pure parsing.
2. **Mapping.** Records → IR. Every vendor parameter becomes a standard IR
   parameter plus, where needed, a **law** (a curve table or formula reference).
3. **Script front end.** Source → the shared script host (section 3).
4. **Evidence.** Native vectors and RE notes proving each law.

A format crate may **not** contain: I/O scheduling, caches, threads, audio
code, script runtimes, UI state, or anything the engine calls back into.
Enforced by dependencies: `format-*` depends on `ir` only (and its reader
libraries), never on `engine`, `stream` or `vm`.

**Laws, not ports.** A vendor's EQ, filter or delay is one of our standard
classes plus a law: a response curve, parameter taper, range, smoothing
time or saturation table. A new DSP class needs a measurement that the
topology itself differs audibly (ENGINE.md "KISS and YAGNI"), recorded next
to the class. Everything else is a law table loaded at prepare time.

Target families (each one class, many laws):

| Family | One class | Laws carried as data |
|---|---|---|
| Filter | ZDF/TPT SVF + ladder (one each) + biquad cascade | slopes, resonance taper, drive curve, cutoff law, mode set |
| EQ | biquad bands (bell/shelf/cut) | Q law, gain range, "analog" proportional-Q curve |
| Dynamics | detector → computer → ballistics → makeup | knee, ratio taper, attack/release law, lookahead |
| Time/mod | modulated delay line + LFO + feedback path | time law, sync table, voice count, interpolation, damping |
| Space | FDN reverb + convolver | size/decay law, early-reflection table, IR |
| Color | oversampled waveshaper + tone stack | transfer table, pre/post EQ, bias |
| Modulators | envelope (multi-segment), LFO, step, follower | curve law, retrigger rules, rate table |

Today's 48.7k lines map onto these seven families. Most of the per-product
files become law tables plus vectors.

## 3. One script host, pluggable languages

"One runtime" does not require one bytecode. It requires **one scheduler and
one host API** with every language executing *inline on the audio thread
under it*.

```
event (sample-stamped) ─▶ script host ─▶ language VM (KSP bytecode | Luau | native)
                              │   ▲
          engine edits (stamped) │   │ waits resume at their sample
          notes via the ledger   │   └── coroutine scheduler (one, sample-accurate)
          UI writes → latest-value store (never queued to audio)
```

Host API (the only surface any language sees): play/stop/fade notes
(`NoteData`-style template → start), event parameters, group allow lists,
engine parameters (applied at their sample), waits and listeners,
transport/time, controller reads, UI store reads/writes, persistent state.

Languages:

- **KSP**: our existing bytecode VM. Keep it; fix the policy
  (run to completion, no fuel abort, UI writes to the store). Run `on init` on
  it and delete `eval.rs`.
- **UVIScript (Lua)**: move Luau **inline onto the audio thread**, which is
  what UVI itself does with Lua 5.1. Callbacks become Luau coroutines resumed
  by the shared scheduler at their exact sample; no script thread, no 1 ms
  polling, offline = real time. Real-time safety: a custom allocator over a
  preallocated arena (mlua exposes a memory limit; a custom arena allocator needs checking against mlua 0.12 or a thin Luau C-API shim), GC
  steps run at callback boundaries with a bounded budget, never inside a
  sample loop. Measure: worst-case GC step and callback time on the UVI corpus.
- **Native script (new libraries)**: decide later with a spike (typed Luau on
  the same host API vs a KODA-style DSL). Either way it gets the same scheduler,
  so there is still one runtime.

## 4. Memory: the sample store

One process-wide **sample store** owns every byte of audio. Engines, parts
and plugin instances borrow from it.

1. **Identity.** A physical sample is keyed by content (file + offset + length +
   format hash). Two parts, two plugin instances or two patches of one library
   share one copy. Kontakt shares inside one instance; sharing across
   instances in one process is a straightforward win.
2. **Native format at rest.** Preloads and pages keep the source encoding (16/24-bit
   PCM; NCW/FLAC either decoded to native PCM, or kept compressed with a
   per-voice decoder, chosen by measurement). Today heads are stereo `f32`:
   2× for 16-bit stereo, 4× for 16-bit mono, before pitch scaling.
3. **The preload law (DFD).** `preload = max(fixed (Kontakt's 60 KB), cold p99
   read latency × max playback step) + the zone's start-modulation range`.
   Script offsets are capped to that range, the way Kontakt caps them with S.Mod.
   A voice never waits; a late page is a counted dropout.
4. **Per-voice stream buffers.** Two swapped buffers per streaming voice
   (HISE), filled by the I/O service ahead of the read head; page cache shared
   by voices on the same sample.
5. **I/O service.** One service, many queues: io_uring (Linux) / overlapped I/O
   (Windows) / dispatch I/O (macOS), O_DIRECT where it helps, deadline-ordered
   requests (the voice's next-buffer time), decrypt and decode on worker
   threads. No format code inside; formats hand it a reader recipe.
6. **Budgets you can see.** RAM shown per instrument and per mic. Purging a
   mic or group drops its preloads (KODA's bus bypass), idle samples are
   reclaimed (sfizz), and the preload slider shows its RAM cost (Opus).
7. **Packages.** Per-instrument and per-mic downloads, LITE variants, and mic
   merge as an offline bake the engine plays like any mic (SINE).

## 5. CPU

1. **Prepare once, run flat.** At load, each voice chain and bus chain compiles
   to a flat list of concrete kernels (enum dispatch, no per-sample virtual
   calls). Bypassed modules cost zero. Topology changes swap a new list in.
2. **Batch by kernel.** Voices sharing a resampler/filter kernel render in SIMD
   lanes (`sampler-simd`, `dsp/lanes.rs` exist; make them the only path, not one
   of ~54 `render*` variants).
3. **Fixed control rate.** Modulation evaluates on an absolute grid (we
   already do: `voice_mod::CELL`) and ramps between points. Results are
   independent of host block size.
4. **Cull and steal.** End inaudible releases (we do), steal by a stated law
   (oldest released, then quietest), and count steals.
5. **Parallel by voice group, deterministic.** Worker threads render disjoint
   voice sets into per-thread buffers summed in fixed order: same output
   whatever the thread count.
6. **Scripts off the hot path.** Callbacks run between render segments,
   never per sample. A heavy `on note` costs its own time once, not a budget
   shared by everything.
7. **Measure what users feel.** Worst-case callback time at 64/128/1024 frames,
   voices per core at a fixed quality, idle cost of a loaded instrument.

## 6. Future-proofing

- **Inputs:** MIDI 2.0 UMP, MPE, CLAP note expressions and polyphonic
  modulation, MTS-ESP tuning: all mapped to one per-note expression model
  in the ledger (we have most of the pieces in `sampler-midi`).
- **Outputs:** bus layouts beyond stereo (surround/Atmos beds) as a mixer
  property, not a special case.
- **Sources:** the IR `Source` grows variants without engine forks: sample,
  wavetable, granular/stretch, oscillator, and later **model/neural** (a voice
  source that renders frames from expression, like Instrument X or SWAM).
  Hybrid instruments: sampled attack + modeled sustain on one note.
- **Formats:** a versioned, open native format (`format-kontra`) and SFZ
  import/export; every vendor format is a translator into the same IR.
- **Platforms:** the engine is `no_std`-friendly at its core (no OS calls on
  audio); WASM builds already run Luau. Nothing in the engine assumes a desktop.
- **Proof:** the contract probe (FUTURE_SAMPLER_PLAN.md §5) is the definition
  of "works". A new format is done when its corpus passes the same contracts.

## 7. Size targets

| Layer | Today | Target |
|---|---|---|
| Kontakt translator (readers + mapping + KSP front end) | 40.3k + 23.9k | readers as needed; mapping < 10k; no stream/cache/runtime |
| UVI translator (readers + mapping + Lua front end) | 36.5k | readers as needed; mapping < 8k; no thread/host runtime |
| DSP | 48.7k, per-product ports | 7 families + law tables + vectors |
| Engine (`sampler-core` minus dsp) | ~86k | one voice path, one ledger, one scheduler |
| Plugin glue (`src/sound/v2.rs`) | 10.2k | < 2k: wiring only |

## 8. Order of work

The first three steps come from the playback audit (on-time start, UI/audio
split, run to completion). Then, for "one engine":

4. **Move the stream service out of `sampler-kontakt`** into `stream/`; UVI
   and Kontakt both hand it reader recipes. (Deletes the UVI → Kontakt dependency.)
5. **Inline Luau** under the shared scheduler; delete `ScriptThread`. Gate:
   UVI offline render = real-time render; scripted onsets at their sample.
6. **One note entry**: scripted and unscripted parts both go through the
   ledger and script host; delete the `part.script` branch in `deliver`.
7. **DSP families**: collapse filters first (largest duplication), then
   time/mod, EQ, color, each behind its native vectors.
8. **Sample store**: content-keyed, native-format, process-wide; then packages.
