# Designing a Universal Multisampler Core in Rust

## Executive conclusion

The most important conclusion from studying **Kontakt 8/KSP, Falcon/UVI Script, DecentSampler, SampleTank 4, HISE, sfizz, MIDI 2.0, VST3, and CLAP** is that a world-class sampler should **not** be designed around “playing sample files.” It should be designed around **events, musical state, selection, voice lifetime, modulation, and deterministic real-time scheduling**. Samples are merely one possible sound source downstream of that machinery.

Kontakt itself demonstrates this distinction explicitly: its documentation describes a **voice** as roughly a sample currently being played, while an **event** corresponds more closely to the note message being processed. KSP then attaches behavior to events through unique event IDs, event-specific group eligibility, transformations, controller callbacks, note generation, and voice manipulation. citeturn19search26turn21search1 Falcon reaches a similar destination through a different architecture: `Synth → Part → Program → Layer → Keygroup → Oscillator`, with event processors and Lua-based UVI Script operating before and around synthesis. citeturn20search1turn20search2

As of October 2026, Kontakt's current KSP documentation identifies Kontakt **8.12**, Falcon's current product is the **Falcon 2026** generation, DecentSampler's current public release/documentation is **1.34.0**, and IK's current sampler workstation remains SampleTank 4. citeturn16search0turn20search0turn17search27turn16search10

My central recommendation is therefore:

> **Build a universal sampler semantics engine first, a sample player second, and format compatibility as adapters around that engine.**

The architecture I would choose is:

```text
                    UNIVERSAL INSTRUMENT SOURCE
                               │
                               ▼
                   ┌─────────────────────────┐
                   │ Semantic Instrument IR  │
                   │ regions / dimensions    │
                   │ selectors / modulation  │
                   │ triggers / voice rules  │
                   └────────────┬────────────┘
                                │ compile
                                ▼
                ┌──────────────────────────────┐
                │ Immutable Runtime Instrument │
                │ indexes + bytecode + tables  │
                └──────────────┬───────────────┘
                               │
Host MIDI/MIDI 2/automation    │
         │                     │
         ▼                     ▼
┌────────────────┐   ┌─────────────────────┐
│ Event Adapter  │──▶│ Musical State Engine│
└────────────────┘   └──────────┬──────────┘
                                ▼
                     ┌─────────────────────┐
                     │ Selector / Trigger VM│
                     └──────────┬──────────┘
                                ▼
                     Activation Set
                                │
                                ▼
                    ┌───────────────────────┐
                    │ Note/Voice Allocator  │
                    └───────────┬───────────┘
                                │
               ┌────────────────┼──────────────┐
               ▼                ▼              ▼
          Sample voice      Stretch voice    Synth voice
               │                │              │
               └────────────────┼──────────────┘
                                ▼
                       Per-voice modulation
                                │
                                ▼
                           Voice DSP
                                │
                                ▼
                      buses / sends / outputs
```

And independently:

```text
                   FORMAT ADAPTER LAYER

 DecentSampler XML ─┐
 SFZ ───────────────┤
 your native format ├──► Universal Semantic IR
 custom converters ─┘

 Universal Semantic IR
          │
          ├──► DecentSampler exporter
          ├──► SFZ exporter
          ├──► Kontakt authoring + KSP generator
          ├──► Falcon mapping + UVI Script generator
          └──► SampleTank-oriented asset/export workflow
```

That separation is the difference between building a sampler that survives ten years and building a sampler whose architecture eventually becomes a collection of `if kontakt_mode`, `if keyswitch`, `if round_robin`, and `if mpe` branches.

The strongest lessons from the products researched are roughly these:

| System | Most important architectural lesson |
|---|---|
| **Kontakt** | Separate event identity from voices; combine static group/zone selection with powerful event scripting and explicit voice-group management. citeturn16search2turn21search1turn21search4 |
| **Falcon** | Separate musical layers/keygroups from actual oscillators, and treat additional selection dimensions as first-class coordinates rather than abusing MIDI channels. citeturn16search6turn20search2 |
| **DecentSampler** | A surprisingly large portion of useful sampler behavior can be represented declaratively: ranges, round robins, bindings, tags, voice muting, legato, and per-voice CC modulation do not inherently require scripting. citeturn18search0turn18search2turn18search3 |
| **SampleTank** | Practical authoring benefits enormously from a clean Element → Zone → Oscillator-style representation, automatic mapping, multiple oscillators per zone, explicit articulation support, and RR at the source level. citeturn16search9turn16search10 |
| **HISE** | Group/dimension selection must be attached to the actual event rather than mutable global state; its event-ID-specific group APIs exist precisely because state can otherwise change before voice allocation consumes it. citeturn19search3 |
| **sfizz** | Regions should largely be passive descriptions, shared resources should be centralized, and voices should come from a reusable pool rather than being dynamically created objects. citeturn19search0turn19search5 |

The resulting Rust engine should be built around six hard invariants:

1. **Every musical note instance has a stable identity.**
2. **Every decision that affects a note is frozen or explicitly versioned at the correct event time.**
3. **The audio callback allocates nothing, waits for nothing, opens nothing, and locks nothing.**
4. **Selection and modulation are different systems.**
5. **Musical semantics are independent of Kontakt, Falcon, SFZ, MIDI 1.0, or a particular plug-in API.**
6. **Compatibility is translated semantically, with explicit reporting of what is exact, approximated, or impossible.**

Steinberg explicitly advises against filesystem access, networking, UI calls, allocation/deallocation, and locking in the real-time VST3 processing path; Apple similarly identifies blocking locks and allocation on the audio path as sources of playback glitches. citeturn22search0turn22search6 Those rules should be treated as architectural constraints, not optimization suggestions.

## What the major multisamplers actually teach us

**Kontakt's fundamental hierarchy is excellent, but its most important feature is not the hierarchy itself.** Zones define sample mappings; Groups collect zones and add playback, effects, modulation, and triggering behavior; Instruments contain groups and higher-level behavior. Kontakt's Group Start Options can select groups based on keys, CC ranges, round robin, random choice, and logical combinations; its current Lua API exposes up to four group-start criteria and up to 4096 groups. citeturn16search2turn16search3

This explains a huge percentage of sophisticated orchestral libraries. A keyswitch is not fundamentally a special type of sampler feature. It is:

```text
incoming note
   │
   ├─ Is this note a control note?
   │        │
   │        └──► mutate articulation state
   │
   └─ Is this a playable note?
            │
            ▼
     evaluate active articulation
            │
            ▼
       choose regions/groups
```

Likewise, a CC switch is just another predicate over musical state, and round robin is a stateful selector. Kontakt's native Group Start Options already expose exactly this conceptual family: **Start on Key**, **Start on Controller**, **Cycle Round Robin**, **Cycle Random**, and logical chaining. citeturn16search3turn16search1

But Kontakt goes significantly beyond static conditions through **KSP**. Native Instruments describes KSP as the technology determining how samples are played, how MIDI interacts with an instrument, and how the instrument behaves. citeturn16search0 Current KSP provides note/release/controller-style callbacks, unique event IDs, generated notes via `play_note()`, event-specific group allow/disallow state, event parameter storage, per-event modulation, and MIDI 2.0 per-note controller support. citeturn21search1turn21search2turn21search3

This distinction matters enormously for your design:

**Kontakt Lua and KSP are not the same layer.** The Kontakt Lua API is explicitly an **instrument-editing and creation API** running inside Kontakt, whereas KSP is the runtime musical/event scripting system. citeturn23search6turn16search0

That suggests your project should also have two different programming layers:

```text
AUTHORING API                       REALTIME EVENT API

create regions                     on_note
map samples                        on_release
set ranges                         on_cc
build UI                           on_note_expression
compile instrument                 emit_note
export                             select regions
asset management                   alter voice
                                   schedule event
```

Trying to make one scripting environment safely do both jobs is unnecessary and dangerous.

Kontakt also shows why **voice allocation must be its own subsystem**. It supports up to 128 configurable voice groups; each can specify a voice count, stealing mode such as oldest/newest/highest/lowest, a steal fade, preference for already-released voices, and an exclusive-group relationship. citeturn21search4 Kontakt's manual explains that Exclusive Groups cause voices belonging to associated Voice Groups to cut one another off, which is exactly the generalized form of open/closed hi-hat muting. citeturn19search1

Then there is playback architecture. Kontakt distinguishes ordinary in-memory **Sampler** playback from **DFD — Direct From Disk** and exposes a DFD preload buffer, alongside Time Machine, Tone Machine, Beat Machine, wavetable, and vintage playback modes. citeturn19search1turn21search14 This is an important clue: **mapping logic should never know which playback algorithm ultimately renders the voice.**

A region should point to a source definition:

```text
Region
  └── SourceId
         ├── SamplePlayback
         ├── TimeStretch
         ├── Granular
         ├── Wavetable
         └── future source...
```

Kontakt's AET system also gives you a useful definition of "morphing." It is not merely crossfading two sample gains. AET analyzes samples spectrally, derives frequency-response fingerprints, and uses a high-resolution FFT filter to impose/interpolate those timbral characteristics onto a currently playing source. It can produce both velocity morphs and articulation morphs. citeturn23search0

So your engine should explicitly distinguish at least three completely different things that users casually call “morphing”:

```text
Layer crossfade
    two or more actual voices are mixed with changing gain

Parameter morph
    one or more synthesis/modulation parameters interpolate

Spectral/timbral morph
    spectral characteristics from another source transform one voice
```

Do not collapse them into one implementation.

**Falcon provides perhaps the best lesson for your universal mapping model.** Falcon uses a semi-modular architecture with sample and synthesis oscillators, modulators, effects, and event processors. Its current engine hierarchy exposed to UVI Script is `Synth → Parts → Program → Layers → Keygroups → Oscillators`; keygroups represent ranges of keys/velocities and can contain multiple oscillators. citeturn20search0turn20search2turn20search4

The most interesting Falcon concept for this project is the **SampleMappingOscillator**. UVI's current mapping model selects zones along **four axes**:

```text
key
velocity
dim1
dim2
```

UVI explicitly describes `dim1` as suitable for things such as articulation, microphone, or dynamic layer and `dim2` as suitable for round-robin variants. Exactly one zone is chosen for a mapping-oscillator event. Critically, UVI deliberately does **not** use MIDI channel as a fallback for `dim1`; dimensions are populated explicitly by event logic. citeturn16search6

That is an extremely strong design choice.

Your universal format should therefore not encode:

```text
MIDI channel 1 = sustain
MIDI channel 2 = staccato
MIDI channel 3 = pizzicato
```

as fundamental architecture.

Instead:

```text
channel = 2
articulation = pizzicato
mic = close
dynamic = mf
rr = 3
```

should be independent state.

Then an importer can translate libraries that *happen* to use MIDI channels as articulation selectors into those explicit dimensions.

Falcon's UVI Script is equally instructive. It is a real-time event-processing language built on Lua 5.1. Its VM operates under real-time constraints; scripts receive callbacks including notes, releases, CCs, pitch bend, polyphonic aftertouch, channel aftertouch, program changes, and transport events. citeturn17search7turn20search5 Events can be forwarded or regenerated, and `playNote`/`postEvent` provide voice IDs used for subsequent voice manipulation. citeturn17search4turn20search6

Falcon also supports event-specific routing through fields including layer, channel, tuning, volume, pan, oscillator index, and—when SampleMappingOscillator is involved—`dim1` and `dim2`. citeturn20search5turn17search5

This reinforces another crucial rule:

> **Every decision concerning a note must either travel with that note or resolve to immutable state attached to its NoteInstance.**

A global variable called `current_articulation` may be useful as high-level state, but when a note is accepted you should capture the resulting articulation into that note's state before voice allocation proceeds.

HISE's documentation gives a concrete reason. HISE provides event-ID-specific group-selection calls because changing an active group globally during a MIDI callback can otherwise race conceptually with the later voice-allocation step; the event-specific version guarantees that the intended group remains tied to the intended event. citeturn19search3

**DecentSampler demonstrates how much can be expressed without code at all.** Its current format includes group/sample mappings, multiple round-robin modes, tagging, voice muting, true-legato building blocks, controller bindings, modulation sources, and per-voice MIDI CC behavior. citeturn18search0turn18search2turn18search3

Its round-robin representation includes sequence length and sequence position, and supports sequential, randomized/no-repeat-style, true-random, and disabled behavior. citeturn18search0 Its tag-based `silencedByTags` mechanism generalizes hi-hat-style choking, and its legato recipe uses the same mechanism to replace sustaining voices with transition behavior. citeturn18search3turn18search4

Its newer MIDI CC modulation architecture is also significant: `channel="voice"` means a voice takes its CC state from the channel that triggered it, which is useful for MPE and other multi-channel expression schemes. DecentSampler explicitly differentiates such temporary, note-scoped modulation from global persistent CC parameter bindings. citeturn18search2

This gives you an excellent semantic distinction to copy:

```text
CONTROLLER BINDING
CC1 -> global filter knob
persistent parameter/state change

VOICE MODULATION
CC74 on this voice's channel -> this note's timbre
per-note / per-voice expression
```

They are not interchangeable.

**SampleTank 4**, meanwhile, provides a pragmatic hierarchy worth borrowing for authoring. Its Instrument Editor can automap samples from filenames into **Oscillators, Zones, and Elements**; a Zone covers a note/key range and can contain up to six oscillators, while oscillators can round-robin sequentially or randomly. Elements collect groups of Zones and support trigger behavior such as note-on, note-off, and latch. citeturn16search9 SampleTank's current specification also advertises multi-articulation/key-switch control and MIDI Learn. citeturn16search10

I did **not** find in IK's public current documentation a runtime author scripting environment comparable to KSP or UVI Script. The public authoring story located in this research is centered instead on the SampleTank Instrument Editor, imported samples, Elements/Zones/Oscillators, and engine parameters. That means SampleTank interoperability should initially be treated as an **asset/authoring adapter problem**, rather than assuming there is a public KSP-like programmable runtime you can target automatically. citeturn16search9turn16search10

The result of this comparison is quite clear:

> Your native architecture should be **more abstract than any of them**, while retaining constructs that can be lowered into each of them.

## The universal semantic core

The heart of your sampler should be a compact semantic model with a deliberate separation between **musical events, musical state, selection rules, note instances, and audio voices**.

The most important internal object is not `Voice`.

It is:

```rust
NoteInstance
```

A NoteInstance represents one logical note-producing action.

Conceptually:

```rust
struct NoteInstance {
    id: NoteInstanceId,

    port: u16,
    midi_group: u8,
    channel: u8,

    key: u8,
    velocity: f32,
    release_velocity: f32,

    articulation: ArticulationId,
    selector_snapshot: SelectorSnapshot,

    started_at: u64,
    released_at: Option<u64>,

    key_is_down: bool,
    sustain_held: bool,
    sostenuto_held: bool,

    child_voices: VoiceSpan,
}
```

This object is deliberately separate from the voices it produces.

One note might trigger:

```text
main sustain sample
+ close mic
+ room mic
+ noise layer
+ resonance sample
```

That is **one NoteInstance, five voices**.

Conversely, a legato implementation may reuse one voice across multiple incoming notes while still needing separate note-event identities.

That distinction mirrors the direction taken by Kontakt, where event IDs exist independently from the concept of the actual sounding voice. citeturn19search26turn21search1 It also matches Falcon/UVI Script's use of event/voice IDs for later manipulation. citeturn20search5turn17search4

Your normalized input event should preserve substantially more information than old-fashioned MIDI:

```rust
enum MusicalEvent {
    NoteOn {
        offset: u32,
        port: u16,
        group: u8,
        channel: u8,
        external_note_id: Option<u32>,
        key: u8,
        velocity: f32,
        attribute: Option<NoteAttribute>,
    },

    NoteOff { /* same identity fields */ },

    ControlChange { /* ... */ },
    PitchBend { /* ... */ },
    ChannelPressure { /* ... */ },
    PolyPressure { /* ... */ },

    PerNoteExpression {
        note_id: NoteInstanceId,
        expression: ExpressionKind,
        value: f32,
    },

    ProgramChange { /* ... */ },
    Transport { /* ... */ },
}
```

This is not future-proofing theater. MIDI 2.0 explicitly expands the model, and the MIDI Association's current Orchestral Articulation Profile defines articulation information carried directly by MIDI 2.0 Note On attributes. The MIDI Association also defines an MPE Profile. citeturn17search12turn17search21 Kontakt itself now exposes callbacks and commands for MIDI 2.0 registered/assignable per-note controllers and per-note pitch bend. citeturn21search0turn21search2

Therefore **articulation must be a first-class semantic field**, not permanently synonymous with "the last keyswitch."

A note could obtain its articulation from:

```text
MIDI 2 orchestral-articulation attribute
       ↓
host articulation API
       ↓
keyswitch
       ↓
CC range
       ↓
program change
       ↓
MIDI channel convention
       ↓
script
       ↓
default articulation
```

You decide precedence at the instrument level.

The core selector model should then work on typed predicates.

For example:

```text
Region: Violin_Spiccato_RR3_Forte_G3

key             55 .. 57
velocity        96 .. 127
articulation    spiccato
mic             close
round_robin     3
trigger          attack
channel          any
```

or:

```text
Region: Violin_Sustain_P_ExpressionLayer

key             55 .. 57
velocity        any
articulation    sustain
cc1             0 .. 42
dynamic_dim     p
trigger          attack
```

or:

```text
Region: Legato_G3_to_A3

trigger          legato_transition
previous_key     55
current_key      57
articulation     legato
velocity         1 .. 127
```

This should compile to something like:

```text
STATIC FILTERS
    key
    velocity
    channel/port mask
    trigger type

DYNAMIC SELECTORS
    articulation
    CC ranges
    dimensions
    pedal state
    previous key
    held-note state
    RR
    probability
    script-produced flags
```

You absolutely do not want to linearly scan 50,000 regions for every Note On.

At compile time, build a key-index:

```text
key 0   -> candidate span / bitset
key 1   -> candidate span / bitset
...
key 127 -> candidate span / bitset
```

Then optionally subdivide by trigger and velocity bucket.

After that, dynamic predicates operate only on the surviving candidates.

For very large libraries, I would compile selection into bitsets:

```text
key candidates
      AND velocity candidates
      AND articulation candidates
      AND channel candidates
      AND mic candidates
      AND trigger candidates
      ↓
dynamic condition check
```

HISE's newer complex group-management system is a useful real-world validation of this idea: it repurposes its group dimension as encoded state and performs fast matching using bit masks, including custom round-robin and keyswitch behavior. citeturn19search35

There is an important conceptual difference here between **selection** and **blending**.

Selection says:

```text
Which source regions are eligible?
```

Blending says:

```text
At what gain should each eligible source currently sound?
```

Do not use eligibility predicates for continuous dynamics.

For example, a classical orchestral patch may have:

```text
dynamic dimension:
p
mf
f
ff
```

and CC1 = 0.47.

The selector may enable `mf` and `f`; a continuous blend function produces their weights.

For two adjacent layers, an equal-power pair could be:

```text
gA = cos(πx / 2)
gB = sin(πx / 2)
```

while a linear blend or custom author-defined curve can also be supported.

This allows the same semantic system to express both Kontakt-style velocity-zone behavior and continuously crossfaded dynamic layers.

Your `Region` should consequently be mostly immutable data:

```rust
struct Region {
    source: SourceId,

    key_range: RangeU8,
    velocity_range: RangeF32,

    root_key: f32,
    fine_tune_cents: f32,

    trigger: TriggerKind,

    selector_program: SelectorProgramId,
    blend_group: Option<BlendGroupId>,
    rr_group: Option<RoundRobinGroupId>,

    voice_group: VoiceGroupId,
    choke_group: Option<ChokeGroupId>,

    amp: RegionAmp,
    playback: PlaybackConfig,
}
```

The source should be separate:

```rust
enum SourceDefinition {
    Sample(SampleSource),
    Stretch(StretchSource),
    Granular(GranularSource),
    Wavetable(WavetableSource),
    // future
}
```

Falcon's architecture strongly supports this separation: a Keygroup carries mapping/voice behavior while one or more Oscillators provide the actual sound source, and Falcon's oscillators span conventional sample playback and multiple synthesis methods. citeturn20search0turn20search4

For round robin, do not implement merely:

```rust
rr = (rr + 1) % count;
```

Your universal RR definition should have:

```text
policy:
    cycle
    random
    random_no_repeat
    shuffled_bag
    weighted_random
    explicit_index

reset:
    never
    transport_start
    after_timeout
    articulation_change
    keyswitch
    note
    bar

scope:
    instrument
    layer
    articulation
    key
    key_range
    channel
    custom domain

seed:
    deterministic
    session-random
```

DecentSampler already distinguishes sequential, random-with-nonrepeat behavior, true random, and always/no-RR operation. citeturn18search0 Falcon similarly has first/cycle/random policies for its mapping dimension. citeturn16search6

For professional use, **deterministic randomness is extremely important**. An offline bounce should not unpredictably choose different samples every render unless the patch explicitly requests nondeterminism.

Use a deterministic random stream attached to a semantic domain:

```text
seed = hash(
    instrument_seed,
    rr_group_id,
    sequence_epoch
)
```

A saved project can then replay the same sequence.

Key switching should likewise become a generic `ArticulationStateMachine`, not special-case logic.

Support at least:

```text
latch
momentary
one-shot-next-note
toggle
previous / next
velocity-select
range-select
press/release pair
```

and explicitly define scope:

```text
global instrument
part
MIDI port
MIDI channel
MPE zone
custom selector domain
```

The keyswitch itself should have a consumption mode:

```text
consume        // control-only note
pass_through   // also allow mapped sound
transform      // replace with generated event
```

CC switching should support both range state and edge triggering:

```text
cc1 0..31       = p
cc1 32..63      = mf
cc1 64..95      = f
cc1 96..127     = ff
```

but also:

```text
on CC64 crossing upward through 64 -> pedal_down trigger
on CC64 crossing downward          -> pedal_up trigger
```

For discrete CC decisions, add optional hysteresis so minor controller jitter around a threshold does not repeatedly change articulation.

Velocity must also be split conceptually into multiple roles:

```text
note selection velocity
amp-response velocity
modulation velocity
release velocity
velocity-crossfade position
```

They may begin with the same input value, but they should not be the same engine variable.

The same principle applies to channels:

> **MIDI channel is routing/state metadata, not voice identity and not articulation identity.**

UVI's explicit refusal to use channel as an implicit dimension is a particularly good precedent here. citeturn16search6

## Voices, streaming, morphing, and the sound engine

I would design the engine around a hierarchy of **NoteInstance → VoiceBundle → Voice**.

```text
NoteInstance
│
├── Voice: close mic
├── Voice: room mic
├── Voice: release resonance
└── Voice: sympathetic layer
```

This gives you several advantages.

When global polyphony pressure requires stealing a note, you can often steal the **bundle**, rather than arbitrarily deleting one microphone layer while leaving another. sfizz's historical behavior is instructive here: its project notes that voice stealing can kill all voices started by the same triggering event by default, keeping layered notes coherent. citeturn19search14

Each actual voice slot should be reusable and preallocated:

```rust
struct VoiceSlot {
    generation: u32,
    state: VoiceState,

    note_id: NoteInstanceId,
    source_id: SourceId,
    voice_group: VoiceGroupId,

    cursor: PlaybackCursor,
    envelope: EnvelopeState,
    modulation: VoiceModState,

    current_gain: f32,
    estimated_energy: f32,

    started_at: u64,
    released_at: Option<u64>,
}
```

Use `(slot_index, generation)` as an internal handle rather than a raw slot number. The generation prevents a stale handle from accidentally modifying an unrelated newer voice after slot reuse.

Voice allocation should be a configurable scoring problem.

A sensible default steal order is approximately:

```text
already silent/released
    >
very low envelope energy
    >
release-stage voice
    >
oldest sustained voice
    >
newest/highest/lowest according to group policy
```

Kontakt demonstrates that professional instruments genuinely need configurable policies: current Voice Groups expose count, fade time, released-note preference, exclusive grouping, and oldest/newest/highest/lowest-style modes. citeturn21search4

You should have at least three polyphony limits:

```text
global engine voice limit
instrument/part voice limit
voice-group limit
```

plus choke/exclusive groups.

Do not hard-kill ordinary stolen voices unless specifically requested. Transition them through a short steal ramp:

```text
Active
  │
  ├── normal NoteOff -> Release
  │
  └── voice stolen   -> StealFade -> Dead
```

This prevents discontinuities.

For drums, provide a separate **choke relationship**:

```text
open_hat ─┐
half_hat ─┼── choke family "hat"
closed_hat┘
```

The target can define:

```text
hard cut
short fade
normal envelope release
custom fade time
```

DecentSampler exposes a directly analogous design through tags, `silencedByTags`, immediate/normal silencing, and explicit silencing decay. citeturn18search0turn18search4

Legato deserves its own engine subsystem rather than being faked exclusively through scripts.

Track at minimum:

```text
physical keys currently down
notes sustained by pedals
ordered key history
previous musical note
current musical note
legato overlap state
legato interval
direction
elapsed time since previous event
```

Then trigger semantics can include:

```text
attack
release
first
legato
legato_transition
retrigger
pedal_down
pedal_up
sostenuto_capture
```

DecentSampler's current architecture already distinguishes first/legato-like logic and uses voice-silencing relationships to implement transition behavior. citeturn18search3 SampleTank's Elements expose note-on, note-off, and latch trigger behavior as another example of trigger type being independent of mapping. citeturn16search9

For sophisticated sampled legato, represent transition regions using the **source and destination notes**:

```text
transition:
    from = G3
    to   = A3
```

rather than pretending they are ordinary A3 samples.

The runtime can then decide whether to:

```text
play transition overlay
replace sustain voice
crossfade sustain
retune existing voice
delay destination sustain until transition point
```

depending on the library.

Sustain and sostenuto should also be core voice-state features, not scripts.

Maintain separate state:

```text
key physically down
note logically held by sustain
note captured by sostenuto
audio voice currently sounding
```

Those are not equivalent.

### Streaming

The streaming architecture is where “least CPU” and “least memory” become competing objectives.

Kontakt's DFD mode keeps a preload portion of a sample in memory so playback can begin immediately while the remainder streams from disk. citeturn19search1 HISE similarly documents two per-voice streaming buffers that swap between disk filling and playback. citeturn19search17 sfizz describes an on-demand streaming system that dynamically reclaims sample memory. citeturn19search5

Your architecture should therefore resemble:

```text
                  Sample Metadata
                       │
                       ▼
                 Attack Preload
                       │
             audio thread starts here
                       │
                       ▼
Voice cursor ───────► Decoded Page Cache
                            ▲
                            │
                     background workers
                            ▲
                            │
                       File / archive
```

I would not make one giant ring buffer per sample.

Instead use:

```text
SampleId
   +
PageIndex
   =
CacheKey
```

and maintain a **shared decoded page cache**.

This matters when 50 voices play overlapping portions of the same source or when microphone/layer architecture produces repeated reads.

A sample metadata structure could contain:

```rust
struct SampleMeta {
    file_id: FileId,
    channels: u8,
    sample_rate: u32,
    frames: u64,

    attack_preload: AudioSpan,

    seek_table: SeekTableId,

    loops: LoopSpan,
    root_pitch: f32,
}
```

A voice contains only:

```text
SampleId
playback position
pitch ratio
current cache page
interpolation history
```

The audio thread does **not** ask the filesystem to read anything. It posts bounded page requests to an I/O queue and either consumes ready data or invokes a predefined underflow policy.

Underflow must itself be designed:

```text
silence missing section
short fade to silence
continue from emergency fallback buffer
report underrun telemetry
```

Never wait.

Steinberg explicitly says filesystem access and blocking mechanisms must be avoided in the VST3 real-time processing function. citeturn22search0

A fixed universal preload is also wasteful.

For example, 20,000 mono samples with only 4,096 float frames preloaded would consume roughly **312.5 MiB** before metadata and overhead; stereo doubles that to roughly **625 MiB**.

So preload should be:

```text
author hint
× device behavior
× sample format
× streaming latency history
× current voice pressure
```

with min/max bounds.

Add purge behavior from the beginning. Kontakt and Falcon both expose library/sample-loading concepts where unnecessary material can be removed from active memory; Falcon's current UVI Script engine API includes loading and purging access as part of the synthesis hierarchy. citeturn20search2

For your own eventual library container, I would use:

```text
manifest
content hashes
sample metadata
independently seekable compressed chunks
per-sample seek index
alignment suitable for bulk reads
```

rather than a single compression stream across the entire library. Background workers can decode those chunks without blocking the audio thread.

Initially, though, do not waste a year designing your proprietary container. Support ordinary WAV/AIFF plus one seekable lossless format and prove the runtime first.

### Sample playback

The first playback engine needs:

```text
sample start
sample end
start offset
forward playback
reverse playback

one-shot
gate
sustain loop
release loop
forward loop
ping-pong loop
loop crossfade

root pitch
key tracking
fine tune
per-note pitch
pitch bend
```

Then add playback-quality tiers:

```text
draft
live
high
offline
```

Do not wire interpolation into voice logic. Make the resampler its own kernel:

```text
Voice
  └── PlaybackCursor
         └── ResamplerKernel
```

A production engine can then swap interpolation algorithms without changing region or voice semantics.

### Morphing and continuous dynamics

For ordinary velocity/CC morphing, start with **multi-voice crossfading** because it is predictable and format-portable.

Represent a blend set:

```rust
struct BlendSet {
    dimension: DimensionId,
    members: Range<BlendMember>,
    normalization: BlendNormalization,
}
```

Each member has a center/range/curve.

At CC1 = `x`, calculate a sparse set of active layers and gains.

Do not activate every velocity layer simultaneously. A well-compiled blend only needs the adjacent or otherwise significant contributors.

Later, implement spectral morphing as an optional DSP module.

Kontakt's AET is useful precisely because it shows that spectral morphing is an effect/analysis subsystem: Kontakt analyzes source spectra into Morph Layers/Maps and uses an FFT-based filter to transform the active sample toward the target timbre. citeturn23search0

So your corresponding architecture should be:

```text
Source Voice
     │
     ▼
SpectralMorphProcessor
     ▲
     │
offline-generated timbre descriptors
```

not:

```text
region selector somehow "morphs"
```

### DSP topology

The universal DSP graph should distinguish:

```text
VOICE SCOPE
envelope
per-note filter
pitch
sample start
per-note modulation
per-note distortion if required

GROUP/LAYER SCOPE
shared EQ
shared compression
shared effects

BUS / INSTRUMENT SCOPE
reverb
master EQ
limiting
send effects
```

Anything duplicated per voice potentially multiplies its cost by active polyphony.

This is one of the biggest CPU decisions you will make. A filter running once on a summed bus is radically different from the same filter instantiated for 200 active voices.

Your IR should therefore make scope an explicit type, not an accidental property of where an object happens to be stored.

## MIDI, articulation, modulation, and event processing

The MIDI system should be architected as a **musical event protocol**, not a `uint8_t status/data1/data2` parser.

At your host boundary:

```text
MIDI 1.0
MPE
MIDI 2.0 UMP
VST3 events
CLAP events
future host articulation APIs
```

all convert into your normalized internal events.

VST3 events provide a sample offset, and its Note Expression mechanism supports note-associated expression events inside the process stream. citeturn17search37turn17search35 That means the sampler should process each audio block in segments:

```text
render 0 ........ event A
apply event A

render event A ... event B
apply event B

render event B ... end
```

This is how you get genuinely sample-accurate trigger timing.

Do not quantize note changes to block starts.

The core identity problem deserves special emphasis.

MIDI 1.0 often gives you:

```text
channel + key
```

but that is inadequate for a future-facing engine because overlapping notes of the same key/channel can exist conceptually, while modern plug-in/MIDI models increasingly expose explicit note identity or per-note expression. VST3's example voice interface takes both pitch and `noteId`, and its Note Expression events are designed around note-specific targeting. citeturn17search34turn17search35

Internally, therefore:

```text
NoteInstanceId != MIDI pitch
NoteInstanceId != voice slot
NoteInstanceId != MIDI channel
```

Always.

For legacy MIDI where no explicit note ID exists, allocate your own ID at Note On and maintain the matching policy for subsequent Note Off.

MPE should simply be an **adapter policy**:

```text
MPE member channel
     │
     ├── pitch bend   -> per-note pitch
     ├── pressure     -> per-note pressure
     └── CC74/timbre  -> per-note timbre
```

Falcon's own MPE utility demonstrates exactly this kind of translation: pressure and timbre axes can be routed to polyphonic aftertouch or script-event modulation sources. citeturn20search10 DecentSampler's `channel="voice"` CC modulation similarly reads expression from the triggering voice's MIDI channel. citeturn18search2

MIDI 2.0 should bypass the MPE workaround when genuine per-note data is supplied.

### Articulation engine

I would represent articulation as:

```rust
struct ArticulationState {
    primary: ArticulationId,
    modifiers: BitSet<ModifierId>,
    variation: Option<VariationId>,
}
```

For example:

```text
primary:
    sustain

modifiers:
    con_sordino
    sul_ponticello

variation:
    soft_attack
```

Why not a single integer?

Because real instruments frequently need orthogonal conditions:

```text
legato + con sordino
sustain + sul pont
staccato + muted
```

UVI's explicit extra mapping dimensions strongly support this multidimensional interpretation. citeturn16search6

Then an articulation map can consume many external control schemes:

```text
Source                        → Semantic state

C0 keyswitch                  → articulation = sustain
C#0 keyswitch                 → articulation = staccato
CC32 range                    → articulation = pizzicato
MIDI channel 4                → articulation = tremolo
Program 12                    → articulation = marcato
MIDI 2 Note attribute         → articulation = spiccato
host expression-map event     → articulation = legato
```

The sample engine below this layer does not care which input convention produced that state.

That is how you become “universal.”

### Modulation system

Do **not** make CC numbers your modulation architecture.

Make an abstract signal graph:

```text
Modulation Sources
    velocity
    release velocity
    key tracking
    random per note
    random continuous
    MIDI CC
    pitch bend
    channel pressure
    poly pressure
    MIDI 2 note controller
    MPE pressure
    MPE timbre
    envelope
    LFO
    step sequencer
    macro
    script source
    host automation
    note age
    tempo/transport
```

to:

```text
Targets
    gain
    pan
    pitch
    sample start
    loop location
    filter cutoff
    resonance
    envelope stages
    morph position
    FX parameters
    send amount
    source-specific parameters
```

Every route should specify its scope:

```text
global
part
layer/group
note
voice
```

and its transform:

```text
linear
bipolar
curve
lookup table
quantized
range map
custom compiled function
```

Falcon provides modulation at multiple engine hierarchy levels, with per-voice manipulation and script modulation capabilities, reinforcing the usefulness of scoped modulation rather than one flat modulation table. citeturn20search2turn17search0

DecentSampler's distinction between global controller binding and per-voice MIDI CC modulation is another direct argument for making scope explicit. citeturn18search2

Runtime routes should be compiled into sparse arrays.

Bad:

```text
for every target
    for all 500 possible modulation sources
        if connected...
```

Better:

```text
cutoff.routes = [
    velocity_route,
    cc74_route,
    env2_route
]
```

Only evaluate the connections that exist.

Likewise, distinguish **audio-rate** and **control-rate** modulation.

An ADSR controlling gain may need per-sample accuracy.

A UI macro controlling microphone balance often does not.

For slower modulation, calculate targets at small control blocks and interpolate:

```text
start value ---- ramp ---- end value
```

rather than reevaluating an entire modulation graph per sample.

### Controller state

Maintain a proper controller state table per routing domain:

```text
Port
  └── MIDI Group
       └── Channel
            ├── CC[128]
            ├── pitch bend
            ├── channel pressure
            ├── program
            ├── bank
            ├── pedals
            ├── RPN state
            └── NRPN state
```

Then Note On captures whatever state is required by that patch.

You should distinguish:

```text
LIVE STATE
"What is CC1 right now?"

NOTE START SNAPSHOT
"What was CC1 when this note started?"

CONTINUOUS VOICE STATE
"Follow future CC1 changes for this voice."
```

A selector commonly wants the second.

A continuous expression layer commonly wants the third.

A global UI knob wants the first.

Collapsing these semantics is one of the most common ways sampler implementations become brittle.

## Scripting, KSP/UVI compatibility, and the universal format problem

You should **not** begin by embedding Lua and trying to make it do everything.

Most behavior should be declarative.

Think of the native format as:

```text
samples
regions
dimensions
articulations
selector predicates
round robin
blend groups
voice groups
choke rules
triggers
modulation routes
bindings
DSP graph
UI metadata
```

Only behavior that cannot reasonably be represented by those primitives should require a script.

DecentSampler is excellent evidence for this philosophy: round robins, mappings, bindings, CC modulation, voice muting, and substantial legato behavior are declarative. citeturn18search0turn18search2turn18search3

The scripting layer should therefore sit **above** the ordinary selector engine:

```text
Incoming Event
      │
      ▼
Built-in semantic preprocessing
      │
      ▼
Optional Event Script
      │
      ├── consume
      ├── transform
      ├── generate event(s)
      ├── modify articulation
      ├── attach event metadata
      └── request selector overrides
      │
      ▼
Selector
```

The script should never directly manipulate arbitrary audio-thread memory.

Expose capabilities:

```text
on_note(event)
on_release(event)
on_cc(event)
on_pitch_bend(event)
on_pressure(event)
on_note_expression(event)
on_program(event)
on_transport(event)

emit_note(...)
release_note(...)
set_event_property(...)
set_articulation(...)
set_dimension(...)
set_voice_parameter(...)
schedule(...)
```

This is close to the proven concepts in KSP and UVI Script without cloning either language. KSP exposes event callbacks, generated notes and per-event manipulation; UVI Script exposes corresponding callbacks, `postEvent`, `playNote`, timing primitives, and voice manipulation. citeturn21search1turn20search5turn17search8

### Do not run an unrestricted general-purpose language naively on the audio thread

UVI can run its Lua 5.1 environment under explicitly engineered real-time constraints, but that does **not** mean dropping an ordinary Lua VM into your callback is automatically safe. UVI's documentation specifically describes its Lua environment as sandboxed/customized for real-time audio and sample-accurate operation. citeturn17search7turn17search9

I would build this in stages.

First:

```text
TriggerGraph / SelectorGraph
```

No runtime language.

Second:

```text
small bounded event bytecode VM
```

with:

```text
fixed memory
no filesystem
no networking
no allocation
bounded stack
bounded event-generation count
bounded instruction budget
sample-offset scheduler
```

Then, later:

```text
Lua-like frontend
       │ compile
       ▼
your bounded VM
```

This allows author convenience without putting a garbage-collected general-purpose runtime directly into your most critical processing path.

You can expose Lua separately for **authoring/offline tasks** with much weaker restrictions:

```text
import folder
find samples
create regions
infer root keys
rename dimensions
build UI
compile/export
```

That directly mirrors Native Instruments' split between runtime KSP and the authoring-oriented Kontakt Lua API. citeturn23search6turn16search0

### KSP semantic translation

Your internal model should deliberately contain enough concepts that common KSP constructs lower cleanly.

| KSP concept | Your internal equivalent |
|---|---|
| `$EVENT_ID` | `NoteInstanceId` / event handle |
| `on note` | `on_note` |
| `on release` | `on_release` |
| `%CC[]` | controller-state query |
| `play_note()` | generated `NoteOnAction` |
| event note/velocity modifications | event transform |
| `allow_group()` / event allow-group | activation mask / selector override |
| event-specific modulation | note/voice modulation value |
| Group Start Options | selector predicates |
| Voice Groups | `VoiceGroup` |
| Exclusive Groups | choke relationships |

Kontakt's per-event `$EVENT_PAR_ALLOW_GROUP` mechanism can control group eligibility for a specific event rather than globally, which is exactly why your selector override must belong to the NoteInstance/event object. citeturn21search1

Kontakt's current MIDI 2.0 `on note_controller` support means your script ABI should also avoid baking itself permanently around seven-bit MIDI 1 CC values. citeturn21search2turn21search3

### UVI Script semantic translation

Similarly:

| UVI Script | Your core |
|---|---|
| `onNote(e)` | `on_note` |
| `postEvent(e)` | forward/transform event |
| `playNote()` | emit new note |
| voice ID | NoteInstance/Voice handle |
| `e.layer` | target semantic layer |
| `e.dim1` | semantic dimension |
| `e.dim2` | semantic dimension/RR |
| `changeTune()` | voice/note pitch action |
| `setSampleOffset()` | source playback-position action |
| `sendScriptModulation()` | script modulation source |

UVI even publishes a current guide for porting conceptual KSP constructs to UVI Script, explicitly highlighting that the two products divide the instrument hierarchy differently. citeturn20search7 That reinforces why **your compatibility boundary should be semantic rather than syntactic**.

### What “Kontakt compatible” should realistically mean

This phrase has several very different meanings.

**Level A — common-source compatibility** is realistic and should be your target.

You author:

```text
MyInstrument.usampler
```

and generate:

```text
Kontakt assets + KSP
Falcon assets + UVI Script/mapping
DecentSampler XML
SFZ
your native runtime instrument
```

This is extremely powerful.

**Level B — import ordinary/unprotected source descriptions** is also realistic where formats or APIs are documented.

DecentSampler's XML is deliberately author-readable. Falcon's current SampleMappingOscillator loads mapping files including `.dmap`, `.xml`, and an SFZ subset. citeturn16search7turn16search6

**Level C — arbitrary native commercial-library binary compatibility** is a very different project.

Do not make “load every commercial Kontakt/Falcon library directly” the architectural prerequisite for version one. Native instruments can contain proprietary engine configuration, script assumptions, custom UI behavior, library/licensing metadata, and other platform-specific constructs. Kontakt's native instrument format is `.nki`, and Native Access participates in downloading/activating NI products and libraries. citeturn21search8turn21search10

For a commercial product, any direct handling of proprietary/protected formats should be approached through documented APIs, permitted formats, licensing, or vendor partnerships rather than making reverse engineering the foundation of the sampler.

### Kontakt export

Kontakt is particularly attractive as an export target because Native Instruments now exposes an official **Kontakt Lua API for programmatic instrument editing/creation**. citeturn23search6

The API exposes operations including:

```text
add groups
set group start conditions
assign voice groups
configure instrument voice groups
set MIDI routing
attach script source
```

and instrument voice-group configuration provides up to 128 entries. citeturn21search6turn21search4

Therefore a strong compatibility workflow would be:

```text
Universal IR
     │
     ├── generate Kontakt Lua authoring script
     │        └── creates mapping/groups/config
     │
     └── generate KSP runtime script
              └── implements behavior not expressible natively
```

That is much cleaner than trying to construct undocumented binary `.nki` files yourself.

### Falcon export

Falcon can similarly be targeted semantically through UVI Script plus mapping data.

Falcon's SampleMappingOscillator provides especially useful translation targets for:

```text
key
velocity
articulation/mic dimension
round-robin dimension
```

and UVI Script can explicitly write `dim1`/`dim2` when forwarding events. citeturn16search6turn17search5

There is, however, an important limitation in the current public API: UVI says a SampleMappingOscillator can be driven by scripts but **cannot be created by a script**; it must already exist in the program. citeturn16search6

So an automated Falcon exporter may need a prepared Falcon template containing the necessary oscillator structure, into which your generated mappings and scripts are loaded.

That is exactly why format exporters must advertise capabilities:

```text
exact
lossy
unsupported
requires template
```

rather than pretending every engine is isomorphic.

### DecentSampler and SFZ

DecentSampler should be among your earliest exporters/importers because its declarative model aligns naturally with a universal IR: group/sample mappings, RR, trigger behavior, tags, bindings, and CC modulation all have public representations. citeturn18search0turn18search2turn18search5

SFZ should also be treated as an important interchange language. sfizz demonstrates how a region-centric description can be compiled into a production voice engine, with a Synth owning the region list, common resources, MIDI state, and a voice pool. citeturn19search0 Falcon's current mapping system can consume a subset of SFZ as well. citeturn16search6

### SampleTank

For SampleTank, the presently documented path is more editor-centered: its Editor imports samples, automaps filenames, and builds Oscillators/Zones/Elements. citeturn16search9

So your first SampleTank interoperability layer should probably be:

```text
Universal IR
   │
   ▼
export sample folder
+ normalized naming
+ mapping manifest
+ optional helper conversion workflow
   │
   ▼
SampleTank Instrument Editor
```

unless IK provides or licenses a deeper authoring SDK.

The important thing is that **SampleTank limitations must not leak backward into your native engine.**

## Rust implementation blueprint

Rust is a very good conceptual fit for this architecture because you can strongly separate ownership-heavy authoring/loading code from tightly controlled real-time runtime data. But Rust by itself does **not** make code real-time safe. `Arc`, containers, destructors, allocation, file I/O, mutexes, logging, and arbitrary third-party code can still violate real-time constraints.

Your crate graph should be something like:

```text
sampler_core
    no plug-in framework dependency
    event model
    selector
    voice allocator
    renderer
    modulation
    buses

sampler_ir
    semantic instrument representation

sampler_compile
    IR -> immutable runtime structures

sampler_stream
    file/cache/decode workers

sampler_script_vm
    bounded event VM

sampler_format_native
sampler_format_sfz
sampler_format_decent
sampler_export_kontakt
sampler_export_falcon

sampler_host_clap
sampler_host_vst3
sampler_host_au

sampler_editor
```

The crucial point is:

> **`sampler_core` must not know what VST3, CLAP, KSP, Falcon, or XML are.**

The plug-in adapters normalize events.

The format adapters normalize instruments.

Then your core is independently testable.

### Runtime memory model

At load time:

```text
parse
validate
resolve resources
build indexes
compile selectors
compile modulation routes
preallocate runtime pools
build preload cache
```

Then publish an immutable:

```rust
CompiledInstrument
```

Conceptually:

```rust
struct CompiledInstrument {
    regions: Box<[CompiledRegion]>,
    samples: Box<[SampleMeta]>,

    key_index: KeyIndex,
    selectors: SelectorProgramBank,

    blend_sets: Box<[CompiledBlendSet]>,
    rr_groups: Box<[CompiledRoundRobin]>,
    voice_groups: Box<[VoiceGroupConfig]>,

    modulation: CompiledModGraph,
    buses: CompiledBusGraph,
}
```

The audio thread should see this as immutable.

When the editor changes a mapping:

```text
UI changes model
      │
      ▼
background compiler
      │
      ▼
new CompiledInstrument
      │
      ▼
atomic publication at safe boundary
```

Do not mutate thousands of region objects underneath the audio thread.

Also be careful with reference-counted destruction. Even dropping an `Arc` can potentially trigger destruction if that thread releases the last reference. A robust RT design can publish a new snapshot atomically but defer destruction of old snapshots to a non-real-time reclamation queue.

### Thread model

I would use:

```text
HOST AUDIO THREAD
    normalized events
    selector execution
    voice allocation
    modulation
    rendering
    mixing

STREAM WORKERS
    file reads
    decompression
    page-cache fill

LOADER/COMPILER WORKER
    parse instruments
    compile selectors
    build mapping indexes
    prepare new snapshots

UI THREAD
    editing
    visualization
    user commands

TELEMETRY / OPTIONAL WORKER
    disk statistics
    voice statistics
    cache statistics
```

The audio thread communicates with workers through fixed-capacity queues.

For Rust, `rtrb` is one available example of the kind of primitive appropriate for SPSC paths: it preallocates a fixed-capacity buffer and documents its reads/writes as lock-free and wait-free after creation. citeturn22search4

Do not interpret that as “use one queue type everywhere.” Different producer/consumer topologies need different structures. But fixed-capacity, bounded, nonblocking communication is the pattern.

### Render loop

A clean callback should look conceptually like:

```rust
fn process(block: &mut AudioBlock, events: &[MusicalEvent]) {
    let mut cursor = 0;

    for event in events {
        let offset = event.sample_offset().min(block.frames());

        render_range(cursor, offset, block);
        apply_event(event);

        cursor = offset;
    }

    render_range(cursor, block.frames(), block);
}
```

The actual implementation should avoid polymorphic allocation and unnecessary branches, but the semantics should stay exactly this simple.

VST3 supports sample-offset event timing, so this architecture maps naturally onto the host's model. citeturn17search37

### Selector compilation

A naive region engine might do:

```text
for all regions:
    test key
    test velocity
    test articulation
    test CC
    ...
```

Never ship that.

Compile:

```text
KeyIndex[128]
     │
     ▼
candidate region IDs
     │
     ├── static mask tests
     │
     └── tiny dynamic selector programs
```

A selector VM instruction set could be extremely small:

```text
TEST_ARTICULATION_EQ
TEST_DIMENSION_EQ
TEST_CC_RANGE
TEST_CHANNEL_MASK
TEST_HELD_COUNT
TEST_PREVIOUS_KEY
TEST_PEDAL
TEST_RR_EQ
TEST_RANDOM_RANGE

AND
OR
NOT
ACCEPT
REJECT
```

Most regions may require only two or three dynamic tests.

Then deduplicate selector programs:

```text
5000 regions
but perhaps only 80 unique predicate programs
```

Store each region's `SelectorProgramId`.

That greatly improves cache locality.

### Voice rendering layout

Do not automatically model active voices as heavyweight individually heap-allocated Rust objects.

Use a fixed pool.

For hot render data, benchmark **structure of arrays** or hybrid layout:

```text
voice_state[]
sample_id[]
position[]
increment[]
gain[]
pan[]
env_level[]
...
```

This makes vectorized/batched rendering easier.

But do not dogmatically force every DSP module into SoA if it destroys maintainability. Keep cold configuration outside hot voice memory.

A useful division is:

```text
HOT
    cursor
    gain
    pitch ratio
    envelope state
    active flags

WARM
    note identity
    modulation accumulators
    loop state

COLD
    immutable sample metadata
    region parameters
    names/tags
```

Never put strings or editor metadata in your audio voice slots.

### CPU strategy

“Lowest CPU” is not achieved through one magic optimization.

The major wins will come from architecture:

```text
do not examine irrelevant regions
do not instantiate unnecessary voices
do not process inactive modulation routes
do not run group/bus DSP per voice
do not resample at offline quality during live playback
do not decode files on audio thread
do not allocate
do not lock
do not recompute immutable data
```

Then optimize inner loops.

Use render kernels specialized by common voice configuration rather than one enormous function containing dozens of unpredictable feature branches.

For example:

```text
sample / no loop / no filter
sample / forward loop / no filter
sample / loop / filter
sample / stereo / crossfade
```

can each have optimized kernels selected when the voice starts.

Avoid per-sample virtual dispatch.

Process in moderate chunks where possible:

```text
16 / 32 / 64 sample mini-block
```

while still honoring event offsets exactly.

### Parallel rendering

Do **not** begin by parallelizing individual voice rendering across arbitrary worker threads.

A good single real-time-thread sampler with efficient streaming can support enormous polyphony.

Parallel voice rendering introduces:

```text
synchronization
wake-up deadlines
cache contention
mix reductions
priority management
host scheduling interaction
```

If you eventually need parallel real-time rendering, make it an optional engine mode with persistent real-time workers and fixed task buffers.

On Apple platforms, Apple explicitly provides Audio Workgroup APIs for plug-ins/apps that create their own real-time threads, including parallel RT workers that share the audio I/O deadline. citeturn22search7turn22search9

But this belongs much later than a correct single-thread render core.

### Plug-in APIs

I would make **CLAP and VST3** first-class host adapters.

CLAP is architecturally attractive for a modern expressive sampler because its design emphasizes a unified event model and per-note modulation/expression. citeturn17search49

VST3 remains essential and exposes note IDs, sample offsets, and Note Expression. citeturn17search34turn17search35

AU follows naturally for macOS.

AAX should be an adapter added when the commercial product requires it rather than allowed to shape `sampler_core`.

Keep all of those outside the core.

### Telemetry

Build instrumentation from day one:

```text
active note instances
active voices
voices stolen
voices choked
selector candidates / event
selector nanoseconds / event
render CPU
disk request count
cache-hit ratio
stream underflows
preload memory
decoded-cache memory
script instruction count
```

Kontakt itself exposes voice/disk-related instrumentation to users, which reflects how important those quantities are in sampler behavior. citeturn19search34turn19search26

You cannot optimize a huge sampler correctly without being able to see exactly where time and memory go.

## The build plan I would actually follow

The first milestone should **not** contain granular synthesis, spectral morphing, convolution, a visual scripting language, encrypted libraries, or 150 effects.

Build the semantic core first.

| Stage | What must exist | What deliberately waits |
|---|---|---|
| **Core Alpha** | WAV/AIFF sources; key/velocity ranges; root pitch; Note On/Off; sample-accurate timing; voices; ADSR; simple loops; basic resampling; fixed voice pool | scripting, fancy UI, proprietary containers |
| **Selector Engine** | keyswitches, CC ranges, channel predicates, articulations, trigger types, RR/random, probability, choke groups, release triggers | full compatibility exporters |
| **Expression Engine** | modulation matrix, pitch bend, pressure, MPE, per-note IDs, MIDI 2-ready normalized events, layer crossfades | spectral morphing |
| **Streaming Engine** | attack preload, page cache, worker I/O, purge, underflow telemetry, huge-library stress tests | custom compression/container |
| **Interchange** | native format, DecentSampler import/export, SFZ import/export | proprietary binary compatibility |
| **Platform Export** | Kontakt Lua + KSP generation; Falcon mapping/UVI Script generation; SampleTank-oriented export workflow | attempts to clone protected native formats |
| **Advanced Engine** | true sampled legato tooling, time stretch, granular, spectral/timbral morph, advanced buses/effects | only after core is measured and stable |

Before calling the Core Alpha finished, I would require the following scenarios to pass.

A block-size test must produce musically identical timing for:

```text
16
32
64
128
256
512
1024
```

sample host buffers.

A repeated-note test must correctly handle:

```text
C4 NoteOn A
C4 NoteOn B
C4 NoteOff ?
```

without confusing the two NoteInstances internally.

An articulation stress test should process:

```text
keyswitch
play note
CC switch
play note
channel switch
play note
MIDI2 articulation
play note
```

and verify that every NoteInstance permanently receives the intended semantic articulation.

A round-robin test should prove:

```text
same input sequence
same stored seed
same output sequence
```

across live and offline renders.

A voice-stealing test should deliberately exceed:

```text
group limit
instrument limit
global limit
```

and confirm that stealing never leaves orphaned mic layers or dangling NoteInstance handles.

A choke test should verify:

```text
open hat
closed hat
```

at every relative sample offset, including both events inside the same host block.

A sustain test must distinguish:

```text
physical key release
sustain-held logical release
actual voice envelope release
release-trigger timing
```

A streaming torture test should inject artificial I/O latency and verify:

```text
audio callback never blocks
cache miss is observable
underrun policy is deterministic
worker eventually recovers
```

A selector fuzz test should generate overlapping regions and compare the optimized compiled selector against a slow reference evaluator.

That last one is particularly important. Your optimized bitsets and selector bytecode must be treated as an optimization of a **simple reference semantic implementation**. Keep both:

```text
ReferenceSelector
    slow
    obviously correct
    tests/debug only

CompiledSelector
    extremely fast
    production
```

Then property-test them against each other with millions of randomly generated instrument states.

Your native instrument schema should also have versioned semantics from the beginning:

```text
format_version
engine_feature_requirements
semantic_version
```

and each export adapter should return a structured compatibility report:

```text
Kontakt export

✓ key/velocity mappings             exact
✓ keyswitch articulations           exact
✓ round robin                       exact
✓ choke groups                      exact
✓ CC dynamic blend                  generated KSP
△ custom selector function          approximated
✗ custom granular source            unavailable
```

That capability-report model is fundamental to the meaning of **universal**.

Universal does not mean pretending all sampler engines are identical.

It means:

> **one canonical musical description, plus honest deterministic translations into engines with different capabilities.**

That canonical representation is your true product.

Kontakt then becomes one backend.

Falcon becomes another backend.

DecentSampler becomes another.

SFZ becomes another.

Your own high-performance Rust renderer is the backend with the richest implementation of the canonical semantics.

And that is ultimately where I would place the project's biggest architectural bet: **your universal instrument IR should be more valuable than the sampler executable itself.**

A mature instrument in that IR could define:

```text
sample assets
mapping
articulations
dimensions
round robins
legato transitions
release behavior
pedal behavior
voice/choke rules
continuous dynamics
modulation
routing
UI metadata
portable event logic
```

and then be rendered natively or lowered into other platforms.

Kontakt's event-centric KSP model, Falcon's explicit dimensions and hierarchical oscillators, DecentSampler's declarative portability, SampleTank's practical zone/oscillator authoring, HISE's event-specific group-selection lessons, and sfizz's clean Region/Resources/Voice-Pool architecture all converge on essentially the same deeper principle: **separate description from execution, event from voice, state from modulation, selection from rendering, and authoring from the real-time engine.** citeturn21search1turn16search6turn18search0turn16search9turn19search3turn19search0

If those boundaries are correct, the later features—keyswitches, CC switching, MIDI-channel articulation conventions, velocity layers, round robins, true legato, MPE, MIDI 2.0, multiple microphones, release samples, morphing, disk streaming, KSP export, UVI Script export, and new future control protocols—become extensions of the model rather than architectural exceptions.

That is the foundation I would build the Rust sampler around.