# Sampler landscape, 2026-10

What each serious sampler does best, what to take from it, and what to avoid.
Vendor claims are marked as such; where a source is dated, it says so.

## The players

### Kontakt 8 (Native Instruments)
- **Strength: the ecosystem.** KSP plus thousands of commercial libraries; the
  de facto format. Kontakt 8 added Tools (MIDI manipulation), Leap, Conflux, a
  bigger Wavetable module and the Komplete Instrument Building Toolkit
  (Komplete Script, Komplete UI for high-DPI); Creator Tools gives Lua for
  *offline* instrument editing.
- **Engine laws worth copying:** DFD with a fixed per-sample preload (60 KB
  default, 6–240 KB) extended by each zone's S.Mod range; KSP callbacks run to
  completion at the event time (only a `while` iteration guard); sample-accurate
  event timestamps; `ignore_event` before the first `wait` decides the original
  note.
- **Weak:** KSP is an untyped 1990s language (no structs, no closures, a
  preprocessor culture of 275k-line generated scripts); single vendor; a UI
  model of absolute-positioned widgets.

### UVI Falcon 3 (UVI)
- **Strength: the modular tree and scripting model.** Any module anywhere
  (program → layer → keygroup → oscillator), IRCAM-grade stretch/granular,
  and UVIScript: Lua 5.1 in a sandbox, every callback a cooperative
  micro-thread, `wait(ms)` documented as sample-accurate, `run`/`spawn` for
  background threads, no allocation in the audio path.
- **Take:** coroutine callbacks with sample-accurate waits; the tree as the
  instrument model; script-as-module (scripts are nodes you can stack).
- **Weak:** Lua 5.1 (old, GC-managed); cooperative loops without a yield
  freeze all note processing.

### HISE (open source, Christoph Hart)
- **Strength: open, compiled, exportable.** HiseScript for events;
  ScriptNode graphs; SNEX and Faust compiled via LLVM JIT in development and
  exported to C++ for the shipped plugin. Streaming: a preload per sample plus
  two swapped buffers per voice; a sample-start-modulated range is loaded whole.
- **Take:** "author in a fast dynamic tier, ship compiled"; preload + double
  buffer streaming; per-event start-offset cap stated up front (65,536 samples,
  because events are 128 bits).
- **Weak:** you build a whole plugin per product, not a player; JUCE-heavy.

### KODA Sampler (KODA, early access 2026)
The newest and most talked-about. NAMM 2026 preview, free early access,
engine docs at v1.0.10.
- **Strength: no-code first.** Round robins, mic mixing, key ranges and
  crossfades without scripts; an Articulation → Variation → Group hierarchy;
  a clip-based Lane Editor; drag-and-drop modulation; **Figma GUI import**;
  "LITE" downloads under 10% of raw size (vendor claim); sync delay
  compensation across instruments; a mic mix view.
- **Scripting:** KodaScript, a small DSL transpiled to **Cmajor**, running
  in the audio engine. Events: `initialize`, `process` (per block), `noteOn`,
  `noteOff`, parameter events coalesced to one call per block, `viewRefresh`
  (12–60 Hz, visuals only). `NoteData` is a template you fill and `start()`;
  per-note setters for delay/length in samples, ms or beats, sample start in
  ms, envelope, target articulation/group/mic/bus. `persist` variables with
  hot-reload reconciliation. Plain Cmajor for custom insert FX.
- **Take:** the authoring model (hierarchy, lanes, Figma, LITE packages), the
  `NoteData` template idea (a note is a value you build, then start), UI state
  as values read by `viewRefresh`, `persist` + hot reload, AI-agent-friendly docs.
- **Avoid:** an author-loop budget of 20,000 steps per block that *truncates*
  the loop (the documented behaviour; it is KONTRA's fuel mistake);
  block-quantized parameter events; insert FX per group, not per note (their
  docs warn a chord gets the last note's cutoff); in streaming mode a start can
  only reach the group's Start Range (the Kontakt S.Mod law; fine if stated).

### Opus (EastWest)
- **Strength: delivery and streaming for huge libraries.** Per-instrument
  downloads with previews; streaming settings that trade RAM for disk
  (vendor: "Never Preload" needs M.2/PCIe RAID 0); OpusScript, designed by
  Wolfgang Schneider (Kontakt's creator); scalable hi-res UI.
- **Take:** download-per-instrument/per-mic; a user-facing preload slider that
  states its RAM cost; a streaming tier designed for NVMe.

### Vienna Synchron Player (VSL)
- **Strength: orchestral ergonomics.** The Dimension Tree (every playing
  style as a navigable tree); a mixer with per-mic delay settings for runtime
  compensation and phase switching; surround up to 9.1; a rebuilt streaming
  engine (vendor: multi-mic lookup "up to 32× faster"; 2018 claim).
- **Take:** articulation as a tree of dimensions (not keyswitch soup);
  per-mic time alignment as a first-class control.

### SINE Player (Orchestral Tools)
- **Strength:** articulation-first UX, optional keyswitches, **Mic Merge**
  (render a mic mix to one stereo source to save RAM/CPU), per-instrument
  downloads from the store. (Sources from 2021.)
- **Take:** mic merge as an offline bake the engine plays like any other mic.

### HALion 7 (Steinberg)
- **Strength:** a sampler and six synthesis types in one tree, plus a Lua
  Script module and MediaBay tag browsing.
- **Take:** synthesis as zone types alongside samples (KONTRA already has
  wavetables); tag-first browsing.

### sfizz / SFZ (open source, BSD-2)
- **Strength:** an open text format anyone can write and diff; sfizz streams on
  demand and reclaims memory of sounds no longer used; a `hint_ram_loading`
  opcode forces RAM.
- **Take:** an open, diffable instrument format; reclaim-on-idle.

### Beyond samples: modeling and neural
- **Pianoteq:** physical model tuned from recordings, tiny footprint, continuous expression.
- **Audio Modeling SWAM / PolySWAM (2026):** physical + behavioural modeling with
  "phase-synchronous sample-morphing"; PolySWAM brings ensembles.
- **Dreamtonics Instrument X (2026):** neural acoustic modeling meant to
  replace orchestral libraries: articulations stackable per note, a dynamics
  lane that changes timbre not just gain, "performance retakes", mic mixer.
  Reviewers: a serious alternative for shaping phrases, not yet a replacement
  for library ecosystems.
- **Take:** the future is *hybrid*: samples for the attack and the identity,
  models or neural layers for transitions, dynamics morphing and footprint.

## What "best" means (the criteria we will measure)

1. **Faithful timing.** Every voice of a note starts at its sample, every time,
   cold cache or warm, live or quantized. (Kontakt, HISE: by construction.)
2. **Expression.** MPE, MIDI 2.0 per-note controllers, CLAP note expressions
   and polyphonic modulation as native inputs, not adapters.
3. **Footprint.** Native-format preloads, compressed packages, per-mic and LITE
   downloads, mic merge. (KODA, Opus, SINE.)
4. **Authoring.** No-code for the 80% (KODA), a modern script for the rest,
   compiled DSP for the edge (HISE, KODA/Cmajor), UI from design tools (KODA/Figma).
5. **Compatibility.** Plays Kontakt and UVI libraries faithfully (our moat;
   no competitor plays the others' formats).
6. **Determinism and testability.** Offline equals real time; every behaviour
   has a probe. (No competitor advertises this; we can.)
7. **Hybrid sound.** Room for modeled/neural layers next to samples.

## Sources

- [KODA docs](https://kodasampler.com/docs), [KODA scripting reference](https://kodasampler.com/docs/scripting-reference), [Synthtopia NAMM 2026 preview](https://www.synthtopia.com/content/2026/01/30/koda-next-generation-sampler-sneak-preview/), [rekkerd: KODA early access](https://rekkerd.org/koda-sampler-now-available-in-early-access-free/), [Music of Sound](https://www.musicofsound.co.nz/blog/koda-sampler)
- [New in Kontakt 8](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/new-in-kontakt-8), [rekkerd: Kontakt 8 launch](https://rekkerd.org/native-instruments-launches-kontakt-8-with-new-creative-tools-leap-framework-more/), [Spitfire: Kontakt DFD buffer](https://support.spitfireaudio.com/en/articles/11815484), [VI-Control: S.Mod and DFD offsets](https://vi-control.net/community/threads/k3-and-sample-start-offset.7770/), [NKSP ignore_event](https://doc.linuxsampler.org/Instrument_Scripts/NKSP_Language/Reference/ignore_event_function)
- [UVIScript timing](https://lua.uvi.net/_time_intro.html), [UVIScript reference](https://lua.uvi.net/_lua_reference.html), [About UVIScript](https://support.uvi.net/hc/en-us/articles/360000047157-About-UVIScript)
- [HISE streaming sampler](https://docs.hise.audio/hise-modules/sound-generators/list/streamingsampler.html), [HISE sampler settings](https://docs.hise.audio/working-with-hise/workspaces/sampler-workspace/sample-settings.html), [HISE forum: preload and buffers](https://forum.hise.audio/post/7), [HISE forum: start offset limits](https://forum.hise.audio/post/8824), [HISE forum: Faust/SNEX](https://forum.hise.audio/post/56033)
- [Opus on KVR](https://www.kvraudio.com/product/opus-by-eastwest), [Opus manual](https://manuals.plus/eastwest-libraries/opus-software-manual)
- [VSL Synchron Player (KVR, 2018)](https://www.kvraudio.com/news-print.php?id=41747), [SINEPlayer 1.0.6](https://news.hummingbirdmedia.com/orchestral-tools-sineplayer-106-adds-aax-support-improved-mic-merge)
- [HALion 7 manual](https://archive.steinberg.help/halion/v7/en/HALion_7_Operation_Manual_en.pdf), [sfizz on Zynthian](https://www.zynthian.org/engines/_engine-list/engine-sfizz)
- [Cmajor](https://cmajor.dev/), [Luau native codegen](https://create.roblox.com/docs/luau/native-code-gen), [Luau July 2024 recap](https://luau.org/news/2024-07-23-luau-recap-july-2024)
- [Instrument X (Synthtopia)](https://www.synthtopia.com/content/2026/08/31/dreamtonics-instrument-x-replaces-big-sample-libraries-with-neural-acoustics/), [Instrument X review](https://arefyevstudio.com/en/2026/09/07/dreamtonics-instrument-x-review/), [SWAM strings (SOS)](https://www.soundonsound.com/reviews/audio-modeling-swam-string-sections), [Audio Modeling + GPU Audio (SOS)](https://www.soundonsound.com/news/audio-modeling-partner-gpu-audio), [Pianoteq](https://www.modartt.com/morphing)
- [CLAP announcement](https://www.attackmagazine.com/news/u-he-and-bitwig-announce-new-open-source-plugin-standard-clap/), [MultitrackStudio MIDI 2.0](https://multitrackstudio.com/midi2.php)
