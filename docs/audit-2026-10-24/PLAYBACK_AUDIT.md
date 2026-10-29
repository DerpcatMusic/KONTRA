# Playback audit, 2026-10-24

Why KONTRA plays notes late, doubled or not at all during DAW playback while a
MIDI keyboard sounds fine, measured on real libraries. The verdict: voice
start time depends on things every mature sampler keeps away from it: disk
speed, the UI thread and a script instruction budget.

## Evidence

Probe: `src/sound/v2.rs` `audit_probe_dense_realtime_playback` (ignored test).
It is the real product path (`V2Loader` → `V2Core`, real-time mode, 128-frame
host-paced blocks, effects drained every block, which is a kinder consumer than
the plugin's 10 Hz loader tick).

```
KONTRA_PROBE_NKI=<nki> [KONTRA_PROBE_PATTERN=live] \
  build-slot cargo test --release --lib audit_probe_dense -- --ignored --nocapture
```

Pattern `dense` = 64 four-note chords as 16ths (DAW-style, quantized).
Pattern `live` = the same 64 steps, one note each (a player).

| Instrument | Pattern | Steps audible | Script stalls | Cold starts | Max onset wait | Effects / note |
|---|---|---|---|---|---|---|
| Areia 16 Violins Spiccato Fast | live | 63 / 64, all in block 0 | 49 | 0 | 0 | ~370 (UI) |
| Areia 16 Violins Spiccato Fast | dense | **0 / 64** | 193 | 0 | 0 | ~100 (UI) |
| Vista 5 Violins | live | not measurable (see note) | 0 | **128 / 128 voices** | 1024 frames (21 ms) | 0 |
| Vista 5 Violins | dense | not measurable (see note) | 0 | **362 / 362 voices** | 1408 frames (29 ms) | 0 |

Head sizing on load: Areia head = 611 output frames (13 ms), and the 1.5 GB
head budget was exceeded (`HeadBudgetExceeded`), so heads were trimmed. Vista
head = 614 frames, within budget, yet every start was cold.

Note: the probe sends no CC1/CC11, so Vista's dynamics-driven layers render
near silent (peak 4e-4). The cold-start and onset-wait counters do not depend
on loudness.

Areia's effect traffic is UI: in its `on note` (16,700 lines) the effectful
calls are `set_control_par` (3408 sites), `set_control_par_str` (710),
`set_key_color` (138), `set_key_type` (132), `set_engine_par` (118),
`set_key_pressed` (90) and `message` (67).

## Root causes, ranked

### 1. Voice starts wait for disk (late, doubled, misplaced attacks)

- A voice whose first frames are not resident starts *silent and frozen* until
  storage arrives, with no upper bound ("must keep its requested offset, even
  when storage takes longer than the old 50 ms deadline"):
  `crates/sampler-core/src/source.rs` `render_starved`, `Cursor::cold`.
- Every sampler voice must have a whole *runway* (= the head length) ready
  before it may start: `src/sound/v2.rs` `sampler_stream_contract` →
  `set_stream_start_runway`, checked in `crates/sampler-core/src/onset.rs`.
- Readiness is judged per **family** (one selection decision, i.e. one
  `play_note`), not per musical note (`onset.rs` `Probe::cohort`). Areia and
  Vista play mic layers and transitions as separate `play_note`s, so the close
  mic starts now and the room mics start when their pages land: the "mono
  first, then wider" double attack.
- Head length comes from read latency measured **at load**, on a warm OS
  cache (`crates/sampler-kontakt/src/stream.rs` `head_frames`, p95 of the
  first probe reads), plus 10 ms slack. At playback the cache is cold and
  the decoders contend, so the head is too short exactly when it matters.
- Heads are stored as stereo `f32` (8 bytes/frame whatever the source format)
  and scaled by the zone's worst pitch step (`max_step` 16 in the product
  policy). That is how ANALOG STRINGS reached 20 GB. Over budget, a binary
  search shrinks every head (`bounded_attack_anchors`) and computes them with
  `include_offsets = false`, dropping sample-start-modulation coverage.
- Script-chosen and controller-driven start offsets are explicitly "a separate
  cold-start case" (`start_ranges_geometry`). Areia drives sample start from
  its script through CC113 → `playPos`; Vista from the release-trigger counter
  and key position. Those are exactly the starts that wait on disk.

**Kontakt:** a fixed DFD preload per sample (60 KB default, 6–240 KB)
plus the zone's S.Mod range; `play_note` offsets in DFD are honoured up to
S.Mod. **HISE:** a preload per sample plus two swapped streaming buffers per
voice; a sample-start-modulated range is loaded whole. In both, a voice
*always starts on time* from RAM. A late disk read is a dropout, never a
shifted note.

### 2. Script UI writes can kill notes

- KSP UI calls (`Coverage::Host`) and every engine change the runtime does not
  apply itself (`Coverage::Effect`, including many `set_engine_par`) go into a
  256-entry outbox (`crates/sampler-core/src/ops.rs` `EFFECT_CAPACITY`, `Op::Emit`).
- The plugin moves them to a 1024-entry queue (`src/plugin.rs` `take_effects`)
  drained by the loader task at ~10 Hz (`control_tick` → `apply_effects`).
- A full outbox makes the callback yield; in real time that is `Abort`
  (`src/sound/v2.rs` `set_behavior_overload(Abort)`), and `fail_behavior`
  releases the note (`crates/sampler-core/src/behavior.rs`).
- In the probe, the same Areia patch plays live and is silent as chords: four
  `on note` callbacks in one block emit more than 256 UI writes.
- Engine changes sent as effects are applied on the UI thread, up to ~100 ms
  late, so a per-note `set_engine_par` before `play_note` lands on a later note.

**Kontakt, UVI, KODA:** UI state is a value the GUI reads when it draws
(latest wins, KODA coalesces parameter events per block). UI never exerts
backpressure on audio.

### 3. Script instruction budget aborts callbacks

- Per host block, all callbacks of a part share `frames × 2048` instructions
  (`behavior_allowance`); exhaustion under `Abort` kills the callback and
  releases its note (`dispatch.rs` `yield_behavior`).
- Not triggered in this probe (budget failures 0), but structurally the same
  as cause 2: dense chords concentrate script work in one block.

**Kontakt:** runs a callback to completion at the event's time; the only guard is
the `while` iteration limit (KONTRA already implements it, `LOOP_LIMIT`).
**UVI:** cooperative micro-threads, sample-accurate `wait`. (KODA truncates
author loops at 20,000 steps per block. That is the same mistake, made visible.)

### 4. Auto-align (only when enabled)

`src/timing.rs` holds notes back only while the transport plays and reports
latency; a host that does not compensate hears DAW playback late. Off by default.

## What is right

- Host events split the block at their sample offsets (`src/plugin.rs` process loop).
- The runtime renders up to each scheduled command (`render.rs` `render_inner`).
- Bounded, allocation-free audio thread; a page cache with decode workers.
- A bytecode VM for callbacks (`sampler-core` behaviors) is the right shape;
  its *policy* (fuel, abort, outbox) is wrong.

## What is wasteful or fragile

- Three KSP execution engines: `sampler-ksp/src/eval.rs` (init tree-walker),
  `lower.rs` (callbacks → bytecode), `luau/` (KSP → Luau). Init and callbacks
  can disagree on one builtin.
- `Runtime` has 109 fields; ~54 `render*` functions in `sampler-core`; 152
  "port v1" references; 479 docs.
- Note ownership is spread over `held`, `consumed_hosts`, native physical
  sources, the aligner's host table and families, which is why
  `V2Core::event`/`enqueue` are pages of special cases.
- Counters instead of guarantees: cold holds, onset-wait, late-attack and
  waiting-voice-replacement counters measure lateness that should be impossible.

## Fix order

1. On-time start guarantee (preload rewrite; delete cold holds/runway/cohorts).
2. UI/audio split (latest-value UI store; engine changes applied in audio at their sample).
3. Run-to-completion callbacks (drop the block budget; keep the loop limit).
4. One note ledger.
5. One script VM (init on the same VM).

The plan for all of this is [FUTURE_SAMPLER_PLAN.md](FUTURE_SAMPLER_PLAN.md);
the competitive survey behind it is [SAMPLER_LANDSCAPE.md](SAMPLER_LANDSCAPE.md).
